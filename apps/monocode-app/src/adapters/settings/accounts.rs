//! Provider usage, credential profiles, notifications, and CLI updates.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use futures::FutureExt as _;
use gpui::{App, Subscription, Task};
use monocode_app::boot::AppServices;
use monocode_core::HarnessId;
use monocode_engine::attention::notification_preferences as prefs;
use monocode_engine::attention::{
    Attention, KvLocalStore, NativeRateLimitFetcher, RateLimitFetcher,
};
use monocode_harness::core::auth::{HarnessLogin, supports_harness_login};
use monocode_harness::core::child::BinaryPathChoice;
use monocode_harness::core::provider_accounts as accounts;
use monocode_layout::tab_groups;
use monocode_settings::display_prefs;
use monocode_view_settings::accounts::host::{
    HarnessUpdateHost, HostTask, NotificationsHost, OnChange, OnHarnessUpdated, ProjectAppearance,
    UsageHost,
};
use monocode_view_settings::accounts::model::*;
use monocode_view_settings::accounts::notification_model as view_prefs;

use super::convert;

#[derive(Clone, Default)]
pub struct AccountsAdapter {
    update_source: Rc<Cell<u64>>,
    dismissed: Rc<Cell<bool>>,
}
impl AccountsAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    fn observe_changes(&self, change: OnChange, cx: &mut App) -> Subscription {
        let change: Rc<dyn Fn(&mut App)> = Rc::from(change);
        let limits = Attention::global(cx).rate_limits.clone();
        let notifier = Attention::global(cx).notifier.clone();
        let a = change.clone();
        let usage = cx.observe(&limits, move |_, cx| a(cx));
        let a = change.clone();
        let notifications = cx.observe(&notifier, move |_, cx| a(cx));
        let (send, receive) = async_channel::bounded(1);
        let kv = AppServices::global(cx).kv.subscribe(move |_| {
            let _ = send.try_send(());
        });
        let task = cx.spawn(async move |cx| {
            while receive.recv().await.is_ok() {
                cx.update(|cx| change(cx));
            }
        });
        Subscription::new(move || {
            drop(usage);
            drop(notifications);
            drop(task);
            drop(kv);
        })
    }

    fn appearance(&self, key: &str, seed: &str, cx: &App) -> ProjectAppearance {
        let mut store =
            monocode_engine::projects::KvAppearanceStore(AppServices::global(cx).kv.clone());
        let mut appearance = tab_groups::TabGroupAppearance::new();
        let logos = appearance.load_tab_group_logos(&mut store);
        let mascots = appearance.load_tab_group_mascots(&mut store);
        let colors = appearance.load_tab_group_colors(&mut store);
        let custom = appearance.load_tab_group_custom_colors(&mut store);
        ProjectAppearance {
            logo: tab_groups::resolve_tab_group_logo(key, Some(&logos)),
            mascot: tab_groups::resolve_tab_group_mascot(key, Some(&mascots)),
            color: tab_groups::resolve_tab_group_color(
                key,
                Some(&colors),
                Some(&custom),
                Some(seed),
            ),
        }
    }

    fn update_source(&self, cx: &mut App) -> u64 {
        if self.update_source.get() == 0 {
            let entity = Attention::global(cx).harness_updates.clone();
            self.update_source
                .set(entity.update(cx, |updates, _| updates.new_source()));
        }
        self.update_source.get()
    }
}

impl UsageHost for AccountsAdapter {
    fn observe(&self, change: OnChange, cx: &mut App) -> Option<Subscription> {
        Some(self.observe_changes(change, cx))
    }
    // `observe_changes` hears every `Kv` write, so a window redraws when any
    // window flips either display preference.
    fn show_remaining_usage(&self, cx: &App) -> bool {
        display_prefs::load_show_remaining_usage(&AppServices::global(cx).kv)
    }
    fn mask_emails(&self, cx: &App) -> bool {
        display_prefs::load_mask_emails(&AppServices::global(cx).kv)
    }
    fn rate_limits(
        &self,
        provider: RateLimitProvider,
        account_id: &str,
        cx: &App,
    ) -> Option<ProviderRateLimits> {
        Attention::global(cx)
            .rate_limits
            .read(cx)
            .all()
            .get(
                &monocode_engine::attention::rate_limits_cache::rate_limits_key(
                    convert(provider),
                    account_id,
                ),
            )
            .cloned()
            .map(convert)
    }
    fn load_rate_limits(
        &self,
        provider: RateLimitProvider,
        account_id: &str,
        force: bool,
        cx: &mut App,
    ) -> Task<ProviderRateLimits> {
        let entity = Attention::global(cx).rate_limits.clone();
        let load = entity.update(cx, |limits, cx| {
            limits.load(convert(provider), account_id, force, cx)
        });
        cx.spawn(async move |_| convert(load.await))
    }
    fn set_rate_limits(
        &self,
        provider: RateLimitProvider,
        account_id: &str,
        value: ProviderRateLimits,
        cx: &mut App,
    ) {
        let entity = Attention::global(cx).rate_limits.clone();
        entity.update(cx, |limits, cx| {
            limits.set(convert(provider), account_id, convert(value), cx)
        });
    }
    fn clear_rate_limits(&self, provider: RateLimitProvider, account_id: &str, cx: &mut App) {
        let entity = Attention::global(cx).rate_limits.clone();
        entity.update(cx, |limits, cx| {
            limits.clear(Some((convert(provider), account_id)), cx)
        });
    }
    fn consume_codex_reset_credit(
        &self,
        credit_id: Option<&str>,
        account_id: &str,
        cx: &mut App,
    ) -> HostTask<CodexRateLimitResetOutcome> {
        let entity = Attention::global(cx).rate_limits.clone();
        let task = entity.update(cx, |limits, cx| {
            limits.consume_codex_reset(credit_id.map(str::to_owned), account_id, cx)
        });
        cx.spawn(async move |_| {
            task.await.map(|outcome| {
                use monocode_engine::attention::rate_limits_fetch::CodexResetOutcome;
                match outcome {
                    CodexResetOutcome::Reset => CodexRateLimitResetOutcome::Reset,
                    CodexResetOutcome::NothingToReset => CodexRateLimitResetOutcome::NothingToReset,
                    CodexResetOutcome::NoCredit => CodexRateLimitResetOutcome::NoCredit,
                    CodexResetOutcome::AlreadyRedeemed => {
                        CodexRateLimitResetOutcome::AlreadyRedeemed
                    }
                }
            })
        })
    }
    fn fetch_pi_usage(&self, provider: PiUsageProvider, cx: &mut App) -> Task<ProviderRateLimits> {
        let services = AppServices::global(cx);
        let fetcher = NativeRateLimitFetcher::new(
            services.data_dir.path.clone(),
            cx.background_executor().clone(),
            Some(services.children.clone()),
        );
        let provider = match provider {
            PiUsageProvider::Anthropic => {
                monocode_engine::attention::pi_usage::PiUsageProvider::Anthropic
            }
            PiUsageProvider::OpenaiCodex => {
                monocode_engine::attention::pi_usage::PiUsageProvider::OpenaiCodex
            }
        };
        cx.background_executor()
            .spawn(async move { convert(fetcher.fetch_pi(provider).await) })
    }
    fn window_visible(&self, cx: &App) -> bool {
        cx.active_window().is_some()
    }
    fn supports_harness_login(&self, harness: HarnessId) -> bool {
        supports_harness_login(harness)
    }
    fn login_harness(
        &self,
        harness: HarnessId,
        account_id: Option<&str>,
        cx: &mut App,
    ) -> HostTask<()> {
        let login = HarnessLogin::new(AppServices::global(cx).children.clone(), "native");
        let future = login.login_harness(harness, account_id);
        cx.background_executor()
            .spawn(async move { future.await.map_err(|e| e.to_string()) })
    }
    fn provider_accounts(&self, provider: HarnessId, cx: &App) -> Vec<ProviderAccount> {
        convert(accounts::provider_accounts(
            &KvLocalStore(AppServices::global(cx).kv.clone()),
            provider,
        ))
    }
    fn selected_provider_account_id(
        &self,
        provider: HarnessId,
        project: Option<&str>,
        cx: &App,
    ) -> String {
        accounts::selected_provider_account_id(
            &KvLocalStore(AppServices::global(cx).kv.clone()),
            provider,
            project,
        )
    }
    fn select_provider_account(
        &self,
        provider: HarnessId,
        project: Option<&str>,
        id: &str,
        cx: &mut App,
    ) {
        accounts::select_provider_account(
            &KvLocalStore(AppServices::global(cx).kv.clone()),
            provider,
            project,
            id,
        );
    }
    fn new_provider_account(
        &self,
        provider: HarnessId,
        label: &str,
        cx: &mut App,
    ) -> Result<ProviderAccount, String> {
        Ok(convert(accounts::new_provider_account(
            &KvLocalStore(AppServices::global(cx).kv.clone()),
            provider,
            label,
        )))
    }
    fn save_provider_account(&self, account: &ProviderAccount, cx: &mut App) {
        accounts::save_provider_account(
            &KvLocalStore(AppServices::global(cx).kv.clone()),
            &convert(account),
        );
    }
    fn rename_provider_account(
        &self,
        provider: HarnessId,
        id: &str,
        label: &str,
        cx: &mut App,
    ) -> Result<(), String> {
        accounts::rename_provider_account(
            &KvLocalStore(AppServices::global(cx).kv.clone()),
            provider,
            id,
            label,
        )
        .map(|_| ())
        .ok_or_else(|| "Account no longer exists".into())
    }
    fn remove_provider_account(&self, provider: HarnessId, id: &str, cx: &mut App) {
        accounts::remove_provider_account(
            &KvLocalStore(AppServices::global(cx).kv.clone()),
            provider,
            id,
        );
    }
    fn remove_provider_account_credentials(
        &self,
        provider: HarnessId,
        id: &str,
        cx: &mut App,
    ) -> HostTask<()> {
        let services = AppServices::global(cx);
        let host = services.host.clone();
        let data = services.data_dir.path.clone();
        let id = id.to_string();
        cx.background_executor().spawn(async move {
            monocode_process::harness::provider_account_remove(
                &host,
                &data,
                provider.to_string(),
                id,
            )
        })
    }
    fn confirm_remove_account(&self, account: &ProviderAccount, cx: &mut App) -> Task<bool> {
        monocode_app::bridge::dialogs::confirm(
            &format!("Remove {} and its saved credentials?", account.label),
            "Remove",
            cx,
        )
    }
    fn account_identities(
        &self,
        accounts: &[ProviderAccount],
        cx: &mut App,
    ) -> Task<HashMap<String, Option<ProviderAccountIdentity>>> {
        let data = AppServices::global(cx).data_dir.path.clone();
        let accounts = accounts.to_vec();
        cx.background_executor().spawn(async move {
            accounts
                .into_iter()
                .map(|account| {
                    let identity =
                        monocode_integrations::account_identity::provider_account_identity(
                            &data,
                            account.provider.to_string(),
                            Some(account.id.clone()),
                        )
                        .ok()
                        .flatten()
                        .map(convert);
                    (identity_key(&account), identity)
                })
                .collect()
        })
    }
    fn project_appearance(&self, key: &str, seed: &str, cx: &App) -> ProjectAppearance {
        self.appearance(key, seed, cx)
    }
}

fn to_engine_mute(mute: view_prefs::Mute) -> prefs::Mute {
    match mute {
        view_prefs::Mute::UntilResumed => prefs::Mute::UntilResumed,
        view_prefs::Mute::Until(v) => prefs::Mute::Until(v),
    }
}
impl NotificationsHost for AccountsAdapter {
    fn observe(&self, change: OnChange, cx: &mut App) -> Option<Subscription> {
        Some(self.observe_changes(change, cx))
    }
    fn preferences(&self, cx: &App) -> view_prefs::Preferences {
        prefs::load_notification_preferences(&AppServices::global(cx).kv)
            .into_iter()
            .map(|(id, pref)| {
                (
                    id,
                    view_prefs::ProjectNotificationPreference {
                        disabled: convert(pref.disabled),
                        muted_until: pref.muted_until.map(|v| match v {
                            prefs::Mute::UntilResumed => view_prefs::Mute::UntilResumed,
                            prefs::Mute::Until(t) => view_prefs::Mute::Until(t),
                        }),
                    },
                )
            })
            .collect()
    }
    fn update_preferences(
        &self,
        ids: &[String],
        patch: &view_prefs::PreferencePatch,
        cx: &mut App,
    ) -> Result<(), String> {
        prefs::update_notification_preferences(
            &AppServices::global(cx).kv,
            &ids.iter().map(String::as_str).collect::<Vec<_>>(),
            &prefs::PreferencePatch {
                disabled: patch.disabled.clone().map(convert),
                muted_until: patch.muted_until.map(|v| v.map(to_engine_mute)),
            },
            Attention::now(cx),
        );
        Ok(())
    }
    fn notification_projects(
        &self,
        paths: &[String],
        cx: &App,
    ) -> Vec<view_prefs::NotificationProject> {
        convert(
            monocode_engine::attention::notification_projects::known_notification_project_selection(
                &AppServices::global(cx).kv,
                &paths.iter().map(String::as_str).collect::<Vec<_>>(),
            ),
        )
    }
    fn project_appearance(&self, key: &str, seed: &str, cx: &App) -> ProjectAppearance {
        self.appearance(key, seed, cx)
    }
}

impl HarnessUpdateHost for AccountsAdapter {
    fn check_for_updates(&self, cx: &mut App) -> Task<Vec<HarnessUpdate>> {
        use monocode_engine::attention::harness_updates::{
            HarnessUpdateDeps, UPDATABLE_HARNESSES, claim_launch_harness_update_check,
            fetch_latest_harness_version, find_harness_updates,
        };
        if self.dismissed.get() || !claim_launch_harness_update_check() {
            return Task::ready(Vec::new());
        }
        let services = AppServices::global(cx);
        let settings =
            monocode_settings::load_app_settings(&services.kv, monocode_core::Platform::current());
        let children = services.children.clone();
        let executor = cx.background_executor().clone();
        let harnesses = UPDATABLE_HARNESSES
            .into_iter()
            .filter(|h| !settings.models.hidden_picker_providers.contains(h))
            .collect();
        let deps = HarnessUpdateDeps {
            harnesses,
            installed_version: Box::new(move |id| {
                let children = children.clone();
                async move {
                    children
                        .inspect_harness_binary(id, BinaryPathChoice::Runtime)
                        .await
                        .map(|v| v.version)
                        .map_err(|e| e.to_string())
                }
                .boxed()
            }),
            latest_version: Box::new(move |id| {
                executor
                    .spawn(async move { fetch_latest_harness_version(id) })
                    .boxed()
            }),
        };
        cx.foreground_executor()
            .spawn(async move { convert(find_harness_updates(deps).await) })
    }
    fn dismiss_updates(&self, _: &mut App) {
        self.dismissed.set(true);
    }
    fn update_cli(&self, harness: HarnessId, cx: &mut App) -> HostTask<()> {
        let children = AppServices::global(cx).children.clone();
        cx.background_executor().spawn(async move {
            children
                .update_harness_cli(harness)
                .await
                .map_err(|e| e.to_string())
        })
    }
    fn installed_version(&self, harness: HarnessId, cx: &mut App) -> HostTask<Option<String>> {
        let children = AppServices::global(cx).children.clone();
        cx.background_executor().spawn(async move {
            children
                .inspect_harness_binary(harness, BinaryPathChoice::Runtime)
                .await
                .map(|v| v.version)
                .map_err(|e| e.to_string())
        })
    }
    fn refresh_catalogs(&self, harness: HarnessId, cx: &mut App) -> Task<()> {
        let services = AppServices::global(cx);
        let registry = services.registry.clone();
        let catalog = services.catalog.clone();
        cx.background_executor().spawn(async move {
            registry
                .refresh_harness_catalogs([harness], true, move |id| catalog.has_live_catalog(id))
                .await
        })
    }
    fn announce_updated(&self, harness: HarnessId, cx: &mut App) {
        let source = self.update_source(cx);
        let entity = Attention::global(cx).harness_updates.clone();
        entity.update(cx, |updates, cx| updates.announce(harness, source, cx));
    }
    fn on_harness_updated(&self, callback: OnHarnessUpdated, cx: &mut App) -> Option<Subscription> {
        let source = self.update_source(cx);
        let entity = Attention::global(cx).harness_updates.clone();
        Some(
            monocode_engine::attention::harness_updates::on_harness_updated(
                &entity,
                source,
                move |id, cx| callback(id, cx),
                cx,
            ),
        )
    }
}
