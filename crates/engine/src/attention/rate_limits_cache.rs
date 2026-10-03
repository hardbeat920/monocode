//! The `RateLimits` entity: provider usage windows per account.
//!
//! Ports src/features/providers/model/rateLimitsCache.ts (the shared
//! snapshot cache, one request per account at a time, and a queued forced
//! refresh), the loading half of `useProviderAccountUsage` in
//! accountUsage.ts, the footer refresh and Codex reset in
//! src/app/shell/UsageFooter.tsx, the Pi usage poll in
//! src/app/shell/PiUsage.tsx, and `usageProviders` and `usageSession` from
//! App.tsx lines 1552-1575.

use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared, join_all};
use gpui::{AppContext, Context, EventEmitter, Task};
use monocode_core::HarnessId;
use monocode_core::block::BlockRole;
use monocode_core::session::Session;
use monocode_harness::core::auth_support::latest_turn_needs_harness_login;
use monocode_harness::core::local_store::LocalStore;
use monocode_harness::core::provider_accounts::{
    DEFAULT_PROVIDER_ACCOUNT_ID, PROVIDER_ACCOUNT_PROVIDERS, ProviderAccount, provider_accounts,
};

use super::Clock;
use super::account_usage::account_usage_key;
use super::pi_usage::{PiUsageProvider, pi_billing_provider};
use super::rate_limits::{
    ProviderRateLimits, RATE_LIMIT_MIN_REFETCH_MS, RATE_LIMIT_POLL_MS, RateLimitProvider,
    RateLimitStatus, error_rate_limits, fetching_rate_limits, idle_rate_limits,
    unavailable_rate_limits,
};
use super::rate_limits_fetch::{CodexResetOutcome, RateLimitFetcher};
use crate::runtime::engine::Engine;

/// A shared, cloneable load.
pub type RateLimitsLoad = Shared<BoxFuture<'static, ProviderRateLimits>>;

/// What changed on `RateLimits`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitsEvent {
    /// A snapshot under this `provider:account` key changed.
    Updated(String),
    /// A Pi usage snapshot changed.
    PiUpdated(PiUsageProvider),
}

/// `keyFor`.
pub fn rate_limits_key(provider: RateLimitProvider, account_id: &str) -> String {
    format!("{}:{account_id}", provider.as_str())
}

/// `usageProviders`: the providers whose usage chip the footer shows for
/// the active session.
pub fn usage_providers(active: Option<&Session>) -> Vec<RateLimitProvider> {
    active
        .and_then(|session| RateLimitProvider::from_harness(session.harness))
        .into_iter()
        .collect()
}

/// `usageSession`: what the footer needs from the active session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSession {
    pub id: String,
    pub harness: HarnessId,
    pub model: String,
    pub auth_required: bool,
    pub provider_account_id: Option<String>,
}

/// `usageSession`. A session that already sent a turn without a stored
/// account ran on the default account.
pub fn usage_session(active: Option<&Session>) -> Option<UsageSession> {
    let active = active?;
    Some(UsageSession {
        id: active.id.clone(),
        harness: active.harness,
        model: active.model.clone(),
        auth_required: latest_turn_needs_harness_login(&active.blocks),
        provider_account_id: active.provider_account_id.clone().or_else(|| {
            active
                .blocks
                .iter()
                .any(|block| block.role == BlockRole::User)
                .then(|| DEFAULT_PROVIDER_ACCOUNT_ID.to_string())
        }),
    })
}

/// The footer's snapshot for an account: the cached one, or a fixed
/// "removed account" state when the conversation's account is gone.
pub fn footer_rate_limits(
    cached: ProviderRateLimits,
    provider: RateLimitProvider,
    account_available: bool,
    now: i64,
) -> ProviderRateLimits {
    if account_available {
        cached
    } else {
        unavailable_rate_limits(provider, "This conversation uses a removed account", now)
    }
}

/// One Pi provider's usage, polled while a view watches it.
struct PiUsageState {
    limits: ProviderRateLimits,
    inflight: bool,
    last_fetch_at: i64,
    watchers: Vec<Weak<()>>,
    polling: bool,
    poll: Option<Task<()>>,
}

/// Keeps the Pi usage poll for one provider running. Dropping the last
/// watch stops it at the next poll.
pub struct PiUsageWatch {
    _token: Rc<()>,
}

/// Provider usage snapshots and the loads behind them.
pub struct RateLimits {
    fetcher: Arc<dyn RateLimitFetcher>,
    store: Arc<dyn LocalStore>,
    clock: Clock,
    snapshots: HashMap<String, ProviderRateLimits>,
    pending: HashMap<String, RateLimitsLoad>,
    queued_refreshes: HashMap<String, (u64, RateLimitsLoad)>,
    next_refresh: u64,
    /// `inflight` in `useProviderAccountUsage`.
    account_loads: usize,
    /// `inflight` in the footer: one refresh or reset at a time.
    footer: Option<Shared<Task<()>>>,
    pi: HashMap<PiUsageProvider, PiUsageState>,
}

impl EventEmitter<RateLimitsEvent> for RateLimits {}

impl RateLimits {
    pub fn new(
        fetcher: Arc<dyn RateLimitFetcher>,
        store: Arc<dyn LocalStore>,
        clock: Clock,
    ) -> Self {
        Self {
            fetcher,
            store,
            clock,
            snapshots: HashMap::new(),
            pending: HashMap::new(),
            queued_refreshes: HashMap::new(),
            next_refresh: 0,
            account_loads: 0,
            footer: None,
            pi: HashMap::new(),
        }
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    // The cache.

    /// `getAllRateLimits`: every snapshot by `provider:account`.
    pub fn all(&self) -> &HashMap<String, ProviderRateLimits> {
        &self.snapshots
    }

    /// `getCachedRateLimits`: the snapshot, or the idle state.
    pub fn get(&self, provider: RateLimitProvider, account_id: &str) -> ProviderRateLimits {
        self.snapshots
            .get(&rate_limits_key(provider, account_id))
            .cloned()
            .unwrap_or_else(|| idle_rate_limits(provider))
    }

    /// The snapshot for an account, if one was loaded.
    pub fn cached(&self, account: &ProviderAccount) -> Option<&ProviderRateLimits> {
        self.snapshots.get(&account_usage_key(account))
    }

    fn publish(&mut self, key: String, value: ProviderRateLimits, cx: &mut Context<Self>) {
        self.snapshots.insert(key.clone(), value);
        cx.emit(RateLimitsEvent::Updated(key));
        cx.notify();
    }

    /// `setCachedRateLimits`.
    pub fn set(
        &mut self,
        provider: RateLimitProvider,
        account_id: &str,
        value: ProviderRateLimits,
        cx: &mut Context<Self>,
    ) {
        self.publish(rate_limits_key(provider, account_id), value, cx);
    }

    /// `clearCachedRateLimits`: one account, or everything.
    pub fn clear(&mut self, account: Option<(RateLimitProvider, &str)>, cx: &mut Context<Self>) {
        match account {
            Some((provider, account_id)) => {
                self.snapshots
                    .remove(&rate_limits_key(provider, account_id));
            }
            None => self.snapshots.clear(),
        }
        cx.notify();
    }

    /// `loadRateLimits`: fetch an account once per app lifetime, or again on
    /// an explicit refresh. A forced refresh during a load runs after it,
    /// and concurrent forced refreshes share one run.
    pub fn load(
        &mut self,
        provider: RateLimitProvider,
        account_id: &str,
        force: bool,
        cx: &mut Context<Self>,
    ) -> RateLimitsLoad {
        let key = rate_limits_key(provider, account_id);
        if let Some(running) = self.pending.get(&key).cloned() {
            if !force {
                return running;
            }
            if let Some((_, queued)) = self.queued_refreshes.get(&key) {
                return queued.clone();
            }
            let id = self.next_refresh;
            self.next_refresh += 1;
            let (done, result) = oneshot::channel();
            let fallback = idle_rate_limits(provider);
            let next: RateLimitsLoad = result
                .map(move |result| result.unwrap_or(fallback))
                .boxed()
                .shared();
            self.queued_refreshes
                .insert(key.clone(), (id, next.clone()));
            let account_id = account_id.to_string();
            cx.spawn(async move |this, cx| {
                running.await;
                let Ok(load) =
                    this.update(cx, |this, cx| this.load(provider, &account_id, true, cx))
                else {
                    return;
                };
                let value = load.await;
                this.update(cx, |this, _| {
                    if this
                        .queued_refreshes
                        .get(&key)
                        .is_some_and(|(queued, _)| *queued == id)
                    {
                        this.queued_refreshes.remove(&key);
                    }
                })
                .ok();
                let _ = done.send(value);
            })
            .detach();
            return next;
        }
        if let Some(cached) = self.snapshots.get(&key).filter(|_| !force) {
            return futures::future::ready(cached.clone()).boxed().shared();
        }

        let previous = self.snapshots.get(&key).cloned();
        self.publish(
            key.clone(),
            fetching_rate_limits(provider, previous.as_ref()),
            cx,
        );
        let fetch = self.fetcher.fetch(provider, account_id);
        let (done, result) = oneshot::channel();
        let fallback = idle_rate_limits(provider);
        let run: RateLimitsLoad = result
            .map(move |result| result.unwrap_or(fallback))
            .boxed()
            .shared();
        self.pending.insert(key.clone(), run.clone());
        cx.spawn(async move |this, cx| {
            let value = fetch.await;
            this.update(cx, |this, cx| {
                this.pending.remove(&key);
                this.publish(key, value.clone(), cx);
            })
            .ok();
            let _ = done.send(value);
        })
        .detach();
        run
    }

    // Account usage (`useProviderAccountUsage`).

    /// `accountsFor`: one provider's accounts, or every provider's.
    pub fn accounts(&self, provider: Option<HarnessId>) -> Vec<ProviderAccount> {
        match provider {
            Some(provider) => provider_accounts(self.store.as_ref(), provider),
            None => PROVIDER_ACCOUNT_PROVIDERS
                .iter()
                .flat_map(|provider| provider_accounts(self.store.as_ref(), *provider))
                .collect(),
        }
    }

    /// `refreshing`: an account load or refresh is running.
    pub fn accounts_refreshing(&self) -> bool {
        self.account_loads > 0
    }

    fn load_targets(
        &mut self,
        targets: Vec<ProviderAccount>,
        force: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        if targets.is_empty() {
            return Task::ready(());
        }
        self.account_loads += 1;
        cx.notify();
        let loads: Vec<RateLimitsLoad> = targets
            .iter()
            .filter_map(|account| {
                let provider = RateLimitProvider::from_harness(account.provider)?;
                Some(self.load(provider, &account.id, force, cx))
            })
            .collect();
        cx.spawn(async move |this, cx| {
            join_all(loads).await;
            this.update(cx, |this, cx| {
                this.account_loads -= 1;
                cx.notify();
            })
            .ok();
        })
    }

    /// The load effect: accounts without a snapshot load once. Call it when
    /// an account list appears and when accounts are added or removed.
    pub fn load_accounts(
        &mut self,
        provider: Option<HarnessId>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let targets = self
            .accounts(provider)
            .into_iter()
            .filter(|account| !self.snapshots.contains_key(&account_usage_key(account)))
            .collect();
        self.load_targets(targets, false, cx)
    }

    /// `refresh`: reload every account.
    pub fn refresh_accounts(
        &mut self,
        provider: Option<HarnessId>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let targets = self.accounts(provider);
        self.load_targets(targets, true, cx)
    }

    // The footer (`UsageFooter`).

    /// The footer is refreshing or using a Codex reset.
    pub fn footer_refreshing(&self) -> bool {
        self.footer.is_some()
    }

    /// New accounts load once; returning to the window reads the snapshot
    /// without another request.
    pub fn load_footer(&mut self, targets: &[(RateLimitProvider, String)], cx: &mut Context<Self>) {
        for (provider, account_id) in targets {
            drop(self.load(*provider, account_id, false, cx));
        }
    }

    /// The footer's refresh button. A refresh already running is shared.
    pub fn refresh_footer(
        &mut self,
        targets: &[(RateLimitProvider, String)],
        cx: &mut Context<Self>,
    ) -> Shared<Task<()>> {
        if let Some(running) = &self.footer {
            return running.clone();
        }
        let loads: Vec<RateLimitsLoad> = targets
            .iter()
            .map(|(provider, account_id)| self.load(*provider, account_id, true, cx))
            .collect();
        let run = cx
            .spawn(async move |this, cx| {
                join_all(loads).await;
                this.update(cx, |this, cx| {
                    this.footer = None;
                    cx.notify();
                })
                .ok();
            })
            .shared();
        self.footer = Some(run.clone());
        cx.notify();
        run
    }

    /// `consumeCodexReset`: use a banked Codex reset, then reload the
    /// account. A failure keeps the snapshot and shows the error.
    pub fn consume_codex_reset(
        &mut self,
        credit_id: Option<String>,
        account_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<CodexResetOutcome, String>> {
        let account_id = account_id.to_string();
        let fetcher = self.fetcher.clone();
        let (done, finished) = oneshot::channel::<()>();
        let tracked = cx.background_spawn(finished.map(|_| ())).shared();
        // With nothing running, the reset counts as the footer's operation
        // at once, as the TypeScript set it before its first await.
        let claimed = self.footer.is_none();
        if claimed {
            self.footer = Some(tracked.clone());
            cx.notify();
        }
        cx.spawn(async move |this, cx| {
            if !claimed {
                // `while (inflight.current) await inflight.current`.
                while let Some(running) = this
                    .update(cx, |this, _| this.footer.clone())
                    .ok()
                    .flatten()
                {
                    running.await;
                }
                this.update(cx, |this, cx| {
                    this.footer = Some(tracked);
                    cx.notify();
                })
                .ok();
            }
            let outcome = match fetcher.consume_codex_reset(credit_id, &account_id).await {
                Ok(outcome) => {
                    let reload = this.update(cx, |this, cx| {
                        this.load(RateLimitProvider::Codex, &account_id, true, cx)
                    });
                    if let Ok(reload) = reload {
                        reload.await;
                    }
                    Ok(outcome)
                }
                Err(message) => {
                    this.update(cx, |this, cx| {
                        let previous = this.get(RateLimitProvider::Codex, &account_id);
                        let value = error_rate_limits(
                            RateLimitProvider::Codex,
                            &message,
                            Some(&previous),
                            this.now(),
                        );
                        this.set(RateLimitProvider::Codex, &account_id, value, cx);
                    })
                    .ok();
                    Err(message)
                }
            };
            this.update(cx, |this, cx| {
                this.footer = None;
                cx.notify();
            })
            .ok();
            let _ = done.send(());
            outcome
        })
    }

    // Pi usage (`PiProviderUsage`).

    /// The Pi provider's snapshot, idle until the first fetch.
    pub fn pi_usage(&self, provider: PiUsageProvider) -> ProviderRateLimits {
        self.pi
            .get(&provider)
            .map(|state| state.limits.clone())
            .unwrap_or_else(|| idle_rate_limits(pi_billing_provider(provider)))
    }

    /// Start polling a Pi provider: now, then every `RATE_LIMIT_POLL_MS`
    /// while the window is visible, at most once per
    /// `RATE_LIMIT_MIN_REFETCH_MS` unless forced.
    pub fn watch_pi_usage(
        &mut self,
        provider: PiUsageProvider,
        cx: &mut Context<Self>,
    ) -> PiUsageWatch {
        let token = Rc::new(());
        let state = self.pi.entry(provider).or_insert_with(|| PiUsageState {
            limits: idle_rate_limits(pi_billing_provider(provider)),
            inflight: false,
            last_fetch_at: 0,
            watchers: Vec::new(),
            polling: false,
            poll: None,
        });
        state.watchers.push(Rc::downgrade(&token));
        if !state.polling {
            state.polling = true;
            state.poll = Some(cx.spawn(async move |this, cx| {
                loop {
                    let alive = this
                        .update(cx, |this, cx| {
                            let Some(state) = this.pi.get_mut(&provider) else {
                                return false;
                            };
                            state.watchers.retain(|watcher| watcher.strong_count() > 0);
                            if state.watchers.is_empty() {
                                state.polling = false;
                                return false;
                            }
                            this.refresh_pi_usage(provider, false, cx);
                            true
                        })
                        .unwrap_or(false);
                    if !alive {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(RATE_LIMIT_POLL_MS as u64))
                        .await;
                }
            }));
        }
        PiUsageWatch { _token: token }
    }

    /// `refresh` in `PiProviderUsage`. Unforced refreshes skip a hidden
    /// window and a fetch younger than `RATE_LIMIT_MIN_REFETCH_MS`.
    pub fn refresh_pi_usage(
        &mut self,
        provider: PiUsageProvider,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let now = self.now();
        let hidden = Engine::hooks(cx).workspace.window_hidden(cx);
        let Some(state) = self.pi.get_mut(&provider) else {
            return;
        };
        if state.inflight {
            return;
        }
        if !force && (hidden || now - state.last_fetch_at < RATE_LIMIT_MIN_REFETCH_MS) {
            return;
        }
        state.inflight = true;
        state.limits = ProviderRateLimits {
            status: RateLimitStatus::Fetching,
            ..idle_rate_limits(pi_billing_provider(provider))
        };
        cx.emit(RateLimitsEvent::PiUpdated(provider));
        cx.notify();
        let fetch = self.fetcher.fetch_pi(provider);
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                let now = this.now();
                if let Some(state) = this.pi.get_mut(&provider) {
                    state.watchers.retain(|watcher| watcher.strong_count() > 0);
                    if !state.watchers.is_empty() {
                        state.limits = result;
                    }
                    state.inflight = false;
                    state.last_fetch_at = now;
                }
                cx.emit(RateLimitsEvent::PiUpdated(provider));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The window came back: watched Pi providers refresh unless they
    /// fetched recently.
    pub fn window_focused(&mut self, cx: &mut Context<Self>) {
        let watched: Vec<PiUsageProvider> = self
            .pi
            .iter()
            .filter(|(_, state)| {
                state
                    .watchers
                    .iter()
                    .any(|watcher| watcher.strong_count() > 0)
            })
            .map(|(provider, _)| *provider)
            .collect();
        for provider in watched {
            self.refresh_pi_usage(provider, false, cx);
        }
    }

    /// Providers with a load running, for tests and diagnostics.
    pub fn pending_keys(&self) -> HashSet<String> {
        self.pending.keys().cloned().collect()
    }
}
