//! Engine package `attention`: what tells the user that something needs
//! them, and what limits them.
//!
//! - `Queues`: follow-ups waiting for the current turn (App.tsx queue
//!   effects and callbacks, messageQueue.ts).
//! - `UsageLimits`: turns stopped at a provider usage limit, with the reset
//!   timer that resumes them (App.tsx, usageLimit.ts).
//! - `RateLimits`: provider usage windows per account, the account usage
//!   loader, and the Pi usage poll (rateLimits*.ts, accountUsage.ts,
//!   piUsage.ts).
//! - `Approvals`: pending approvals and questions across sessions, the
//!   answers, and the provider sign-in prompt (App.tsx, approvalToast.ts).
//! - `Notifier`: OS banners under the mute settings and window focus, the
//!   Dock badge, sounds, and the unseen finished sessions
//!   (notifications/model, sounds.ts, useInputNotifications).
//! - `HarnessUpdates`: the harness CLI update check and broadcast.
//!
//! Start with `Attention::init` (or `Attention::init_native`) after
//! `Engine::init`. Views read the entities from `Attention::global(cx)`.

pub mod account_usage;
pub mod approval_toast;
pub mod approvals;
pub mod harness_updates;
pub mod hooks;
pub mod live_agents;
pub mod notification_preferences;
pub mod notification_projects;
pub mod notifications;
pub mod notifier;
pub mod pi_usage;
pub mod platform;
pub mod queue;
pub mod queues;
pub mod rate_limits;
pub mod rate_limits_cache;
pub mod rate_limits_fetch;
pub mod sound_synth;
pub mod sounds;
pub mod usage_limit;
pub mod usage_limits;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext, Entity, Global};
use monocode_harness::core::child::Children;
use monocode_harness::core::local_store::LocalStore;
use monocode_settings::Kv;

pub use approvals::{Approvals, ApprovalsEvent, ProviderSignInRequest};
pub use harness_updates::{HarnessUpdated, HarnessUpdates};
pub use hooks::{ApprovalRouter, AttentionSubmit, SubmitRequest};
pub use notifier::{Notifier, NotifierEvent};
pub use platform::{AttentionPlatform, NativePlatform};
pub use queues::Queues;
pub use rate_limits_cache::{RateLimits, RateLimitsEvent};
pub use rate_limits_fetch::{NativeRateLimitFetcher, RateLimitFetcher};
pub use usage_limits::UsageLimits;

use crate::runtime::engine::Engine;

/// Epoch milliseconds. Tests pass a clock they move with the executor's.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// The system clock.
pub fn system_clock() -> Clock {
    Arc::new(rate_limits::now_ms)
}

/// `localStorage` for the harness feature models (provider accounts), over
/// `Kv`.
pub struct KvLocalStore(pub Kv);

impl LocalStore for KvLocalStore {
    fn get_item(&self, key: &str) -> Option<String> {
        self.0.get_item(key)
    }

    fn set_item(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.set_item(key, value);
        Ok(())
    }
}

/// What `Attention::init` needs.
pub struct AttentionConfig {
    pub kv: Kv,
    pub platform: Arc<dyn AttentionPlatform>,
    pub fetcher: Arc<dyn RateLimitFetcher>,
    pub clock: Clock,
}

/// The window state attention reads but does not own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttentionFocus {
    /// The focused session of the active tab (`active?.id`).
    pub active_session_id: Option<String>,
    /// The Inbox page is open.
    pub inbox_open: bool,
    /// The session behind the open Inbox Ask, if any.
    pub inbox_ask_session_id: Option<String>,
}

impl AttentionFocus {
    /// `activeSessionId`: the session on screen, the Inbox Ask while the
    /// Inbox is open.
    pub fn visible_session_id(&self) -> Option<&str> {
        if self.inbox_open {
            self.inbox_ask_session_id.as_deref()
        } else {
            self.active_session_id.as_deref()
        }
    }
}

/// The attention entities, as a GPUI global.
pub struct Attention {
    pub queues: Entity<Queues>,
    pub usage_limits: Entity<UsageLimits>,
    pub rate_limits: Entity<RateLimits>,
    pub approvals: Entity<Approvals>,
    pub notifier: Entity<Notifier>,
    pub harness_updates: Entity<HarnessUpdates>,
    pub kv: Kv,
    pub clock: Clock,
    submit: Rc<dyn AttentionSubmit>,
    router: Rc<dyn ApprovalRouter>,
}

impl Global for Attention {}

impl Attention {
    /// Create the entities, install the global, and fill in the runtime's
    /// `AttentionHooks`. `Engine::init` must have run.
    pub fn init(config: AttentionConfig, cx: &mut App) {
        let sessions = Engine::sessions(cx);
        let store: Arc<dyn LocalStore> = Arc::new(KvLocalStore(config.kv.clone()));
        let notifier = cx.new(|cx| {
            Notifier::new(
                config.kv.clone(),
                config.platform.clone(),
                config.clock.clone(),
                &sessions,
                cx,
            )
        });
        let approvals = cx.new(|cx| Approvals::new(&sessions, cx));
        let queues = cx.new(|cx| Queues::new(&sessions, cx));
        let usage_limits = cx.new(|cx| {
            UsageLimits::new(config.fetcher.clone(), config.clock.clone(), &sessions, cx)
        });
        let rate_limits =
            cx.new(|_| RateLimits::new(config.fetcher.clone(), store, config.clock.clone()));
        let harness_updates = HarnessUpdates::new(cx);
        let hooks = Rc::new(hooks::RuntimeAttentionHooks {
            notifier: notifier.downgrade(),
            kv: config.kv.clone(),
        });
        Engine::set_hooks(cx, |engine_hooks| engine_hooks.attention = hooks);
        let noop = Rc::new(hooks::NoopAttentionHooks);
        cx.set_global(Attention {
            queues,
            usage_limits,
            rate_limits,
            approvals,
            notifier: notifier.clone(),
            harness_updates,
            kv: config.kv,
            clock: config.clock,
            submit: noop.clone(),
            router: noop,
        });
        notifier.update(cx, |notifier, cx| notifier.boot(cx));
    }

    /// `init` with the real OS calls and fetches. `data_dir` is the app data
    /// directory; `children` runs the Codex and Grok usage probes.
    pub fn init_native(kv: Kv, data_dir: PathBuf, children: Option<Children>, cx: &mut App) {
        let (clicks, clicked) = async_channel::unbounded::<String>();
        let on_click: monocode_platform::notifications::ClickHandler =
            Arc::new(move |session_id: &str| {
                let _ = clicks.try_send(session_id.to_string());
            });
        let platform = Arc::new(NativePlatform::new(
            monocode_settings::APP_IDENTIFIER,
            "main",
            on_click,
        ));
        platform.install();
        let fetcher = Arc::new(NativeRateLimitFetcher::new(
            data_dir,
            cx.background_executor().clone(),
            children,
        ));
        Self::init(
            AttentionConfig {
                kv,
                platform,
                fetcher,
                clock: system_clock(),
            },
            cx,
        );
        let notifier = Self::global(cx).notifier.downgrade();
        cx.spawn(async move |cx| {
            while let Ok(session_id) = clicked.recv().await {
                let Some(notifier) = notifier.upgrade() else {
                    break;
                };
                cx.update(|cx| {
                    notifier.update(cx, |notifier, cx| {
                        notifier.notification_clicked(&session_id, cx)
                    })
                });
            }
        })
        .detach();
    }

    pub fn global(cx: &App) -> &Attention {
        cx.global::<Attention>()
    }

    pub fn try_global(cx: &App) -> Option<&Attention> {
        cx.try_global::<Attention>()
    }

    /// The submit pipeline hook.
    pub fn submit(cx: &App) -> Rc<dyn AttentionSubmit> {
        Self::try_global(cx)
            .map(|attention| attention.submit.clone())
            .unwrap_or_else(|| Rc::new(hooks::NoopAttentionHooks))
    }

    /// The approval routing hook.
    pub fn router(cx: &App) -> Rc<dyn ApprovalRouter> {
        Self::try_global(cx)
            .map(|attention| attention.router.clone())
            .unwrap_or_else(|| Rc::new(hooks::NoopAttentionHooks))
    }

    /// Fill in the submit pipeline (the submit package or the app).
    pub fn set_submit(cx: &mut App, submit: Rc<dyn AttentionSubmit>) {
        cx.global_mut::<Attention>().submit = submit;
    }

    /// Fill in approval routing (the harness bridge, remote, and workspace).
    pub fn set_approval_router(cx: &mut App, router: Rc<dyn ApprovalRouter>) {
        cx.global_mut::<Attention>().router = router;
    }

    /// The focused session or tab changed. The workspace package calls this.
    pub fn set_focus(cx: &mut App, focus: AttentionFocus) {
        let attention = Self::global(cx);
        let (notifier, approvals) = (attention.notifier.clone(), attention.approvals.clone());
        notifier.update(cx, |notifier, cx| notifier.set_focus(focus.clone(), cx));
        approvals.update(cx, |approvals, cx| approvals.set_focus(focus, cx));
    }

    /// The window gained or lost focus (Tauri's `onFocusChanged`).
    pub fn set_window_focused(cx: &mut App, focused: bool) {
        let attention = Self::global(cx);
        let (notifier, rate_limits) = (attention.notifier.clone(), attention.rate_limits.clone());
        notifier.update(cx, |notifier, cx| notifier.set_window_focused(focused, cx));
        if focused {
            rate_limits.update(cx, |rate_limits, cx| rate_limits.window_focused(cx));
        }
    }

    /// The clock attention reads.
    pub fn now(cx: &App) -> i64 {
        Self::try_global(cx)
            .map(|attention| (attention.clock)())
            .unwrap_or_else(rate_limits::now_ms)
    }
}
