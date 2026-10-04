//! What the usage, account, and notification views need from the app.
//!
//! The views own their interaction state and port the React flows (the
//! footer's one-at-a-time refresh, reset, and sign-in, the Pi poll, the
//! update rows). The data and the side effects come through these traits.
//! The app implements them over the engine's attention package
//! (`RateLimits`, the provider account store, `Notifier`, `HarnessUpdates`)
//! and calls the change callback from `observe` when that data moves.
//! Every method has a harmless default, so a gallery or a test fills in only
//! what it checks.

use std::collections::HashMap;

use gpui::{App, Subscription, Task};
use monocode_core::HarnessId;
use monocode_layout::tab_groups::tab_group_color;

use super::model::{
    CodexRateLimitResetOutcome, DEFAULT_PROVIDER_ACCOUNT_ID, HarnessUpdate, PiUsageProvider,
    ProviderAccount, ProviderAccountIdentity, ProviderRateLimits, RateLimitProvider,
    idle_rate_limits, now_ms, pi_billing_provider,
};
use super::notification_model::{NotificationProject, PreferencePatch, Preferences};

pub use crate::settings::HostTask;

/// Runs when data a view reads has changed, so the view can redraw.
pub type OnChange = Box<dyn Fn(&mut App)>;
pub type OnHarnessUpdated = Box<dyn Fn(HarnessId, &mut App)>;

fn unavailable<T: 'static>() -> HostTask<T> {
    Task::ready(Err("Not available in this build".into()))
}

/// A project's saved look on the rail (`resolveTabGroupLogo`,
/// `resolveTabGroupMascot`, `resolveTabGroupColor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectAppearance {
    /// A custom logo image path.
    pub logo: Option<String>,
    /// The mascot picked in the project menu, by name.
    pub mascot: Option<String>,
    /// The project color as `#rrggbb`.
    pub color: String,
}

impl ProjectAppearance {
    /// No overrides: the color hashed from `seed`.
    pub fn hashed(seed: &str) -> Self {
        Self {
            logo: None,
            mascot: None,
            color: tab_group_color(seed).to_string(),
        }
    }
}

/// The usage footer, the usage chips, the Pi usage chip, and the provider
/// accounts card.
pub trait UsageHost {
    /// `Date.now()`.
    fn now(&self) -> i64 {
        now_ms()
    }

    /// Calls `on_change` whenever usage snapshots or accounts change.
    fn observe(&self, _on_change: OnChange, _cx: &mut App) -> Option<Subscription> {
        None
    }

    // Usage snapshots (rateLimitsCache.ts).

    /// The cached snapshot for one account, if it was ever loaded
    /// (`getAllRateLimits()[key]`).
    fn rate_limits(
        &self,
        _provider: RateLimitProvider,
        _account_id: &str,
        _cx: &App,
    ) -> Option<ProviderRateLimits> {
        None
    }

    /// `loadRateLimits`: once per app lifetime, or again when `force`d.
    fn load_rate_limits(
        &self,
        provider: RateLimitProvider,
        _account_id: &str,
        _force: bool,
        _cx: &mut App,
    ) -> Task<ProviderRateLimits> {
        Task::ready(idle_rate_limits(provider))
    }

    /// `setCachedRateLimits`.
    fn set_rate_limits(
        &self,
        _provider: RateLimitProvider,
        _account_id: &str,
        _value: ProviderRateLimits,
        _cx: &mut App,
    ) {
    }

    /// `clearCachedRateLimits` for one account.
    fn clear_rate_limits(&self, _provider: RateLimitProvider, _account_id: &str, _cx: &mut App) {}

    /// `consumeCodexRateLimitResetCredit`.
    fn consume_codex_reset_credit(
        &self,
        _credit_id: Option<&str>,
        _account_id: &str,
        _cx: &mut App,
    ) -> HostTask<CodexRateLimitResetOutcome> {
        unavailable()
    }

    /// `fetchPiUsage`. Never fails: problems come back in the snapshot.
    fn fetch_pi_usage(&self, provider: PiUsageProvider, _cx: &mut App) -> Task<ProviderRateLimits> {
        Task::ready(idle_rate_limits(pi_billing_provider(provider)))
    }

    /// `document.visibilityState === "visible"`.
    fn window_visible(&self, _cx: &App) -> bool {
        true
    }

    // Sign-in (core/auth.ts).

    /// `supportsHarnessLogin`.
    fn supports_harness_login(&self, _harness: HarnessId) -> bool {
        false
    }

    /// `loginHarness`, for the default profile when `account_id` is `None`.
    fn login_harness(
        &self,
        _harness: HarnessId,
        _account_id: Option<&str>,
        _cx: &mut App,
    ) -> HostTask<()> {
        unavailable()
    }

    // Provider accounts (providerAccounts.ts).

    /// `providerAccounts`: the default account first.
    fn provider_accounts(&self, provider: HarnessId, _cx: &App) -> Vec<ProviderAccount> {
        vec![ProviderAccount {
            is_default: Some(true),
            ..ProviderAccount::new(DEFAULT_PROVIDER_ACCOUNT_ID, provider, "Default account")
        }]
    }

    /// `selectedProviderAccountId` for a project (`None` for no project).
    fn selected_provider_account_id(
        &self,
        _provider: HarnessId,
        _project: Option<&str>,
        _cx: &App,
    ) -> String {
        DEFAULT_PROVIDER_ACCOUNT_ID.into()
    }

    /// `selectProviderAccount`.
    fn select_provider_account(
        &self,
        _provider: HarnessId,
        _project: Option<&str>,
        _account_id: &str,
        _cx: &mut App,
    ) {
    }

    /// `newProviderAccount`: a fresh id with a cleaned label. Not saved yet.
    fn new_provider_account(
        &self,
        _provider: HarnessId,
        _label: &str,
        _cx: &mut App,
    ) -> Result<ProviderAccount, String> {
        Err("Not available in this build".into())
    }

    /// `saveProviderAccount`.
    fn save_provider_account(&self, _account: &ProviderAccount, _cx: &mut App) {}

    /// `renameProviderAccount`.
    fn rename_provider_account(
        &self,
        _provider: HarnessId,
        _account_id: &str,
        _label: &str,
        _cx: &mut App,
    ) -> Result<(), String> {
        Err("Not available in this build".into())
    }

    /// `removeProviderAccount`.
    fn remove_provider_account(&self, _provider: HarnessId, _account_id: &str, _cx: &mut App) {}

    /// `removeProviderAccountCredentials`.
    fn remove_provider_account_credentials(
        &self,
        _provider: HarnessId,
        _account_id: &str,
        _cx: &mut App,
    ) -> HostTask<()> {
        unavailable()
    }

    /// The native "Remove account" confirmation (`ask`). Resolves to true
    /// when the user confirms.
    fn confirm_remove_account(&self, _account: &ProviderAccount, _cx: &mut App) -> Task<bool> {
        Task::ready(false)
    }

    /// `useProviderAccountIdentities`: every account's cached identity, by
    /// `identity_key`. A failed read is `None`.
    fn account_identities(
        &self,
        _accounts: &[ProviderAccount],
        _cx: &mut App,
    ) -> Task<HashMap<String, Option<ProviderAccountIdentity>>> {
        Task::ready(HashMap::new())
    }

    // Project appearance (tabGroups.ts).

    /// The project's logo, mascot, and color. `project_key` is
    /// `projectKey(path)` or a label; `seed` hashes the default color.
    fn project_appearance(&self, _project_key: &str, seed: &str, _cx: &App) -> ProjectAppearance {
        ProjectAppearance::hashed(seed)
    }
}

/// `NotificationMuteControl` and `ProjectNotificationSettings`.
pub trait NotificationsHost {
    /// `Date.now()`.
    fn now(&self) -> i64 {
        now_ms()
    }

    /// Calls `on_change` whenever preferences or the project catalog change.
    /// The views schedule their own redraw for mute deadlines.
    fn observe(&self, _on_change: OnChange, _cx: &mut App) -> Option<Subscription> {
        None
    }

    /// `loadNotificationPreferences`.
    fn preferences(&self, _cx: &App) -> Preferences {
        Preferences::new()
    }

    /// `updateNotificationPreferences`. An error means the store refused
    /// the write.
    fn update_preferences(
        &self,
        _project_ids: &[String],
        _patch: &PreferencePatch,
        _cx: &mut App,
    ) -> Result<(), String> {
        Err("Not available in this build".into())
    }

    /// `knownNotificationProjectSelection(paths).projects`.
    fn notification_projects(&self, _paths: &[String], _cx: &App) -> Vec<NotificationProject> {
        Vec::new()
    }

    /// The project's logo, mascot, and color, as [`UsageHost::project_appearance`].
    fn project_appearance(&self, _project_key: &str, seed: &str, _cx: &App) -> ProjectAppearance {
        ProjectAppearance::hashed(seed)
    }
}

/// `HarnessUpdateNotice`.
pub trait HarnessUpdateHost {
    /// `checkForHarnessUpdates`: the launch check, claimed once per app
    /// launch by whichever window asks first.
    fn check_for_updates(&self, _cx: &mut App) -> Task<Vec<HarnessUpdate>> {
        Task::ready(Vec::new())
    }

    /// Dismissing the notice, for this run only (`launchCheck =
    /// Promise.resolve([])`).
    fn dismiss_updates(&self, _cx: &mut App) {}

    /// `updateHarnessCli`.
    fn update_cli(&self, _harness: HarnessId, _cx: &mut App) -> HostTask<()> {
        unavailable()
    }

    /// `inspectHarnessBinary(harness).version`.
    fn installed_version(&self, _harness: HarnessId, _cx: &mut App) -> HostTask<Option<String>> {
        unavailable()
    }

    /// `refreshHarnessCatalogs([harness], { force: true })`.
    fn refresh_catalogs(&self, _harness: HarnessId, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }

    /// `announceHarnessUpdated`: tells the other windows.
    fn announce_updated(&self, _harness: HarnessId, _cx: &mut App) {}

    /// `onHarnessUpdated`: `on_update` runs when another window updated a
    /// harness. The host filters out this window's own announcements.
    fn on_harness_updated(
        &self,
        _on_update: OnHarnessUpdated,
        _cx: &mut App,
    ) -> Option<Subscription> {
        None
    }
}

/// Every default, for galleries and tests.
#[derive(Default)]
pub struct NoopAccountsHost;

impl UsageHost for NoopAccountsHost {}
impl NotificationsHost for NoopAccountsHost {}
impl HarnessUpdateHost for NoopAccountsHost {}
