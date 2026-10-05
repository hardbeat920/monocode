//! Port of src/app/shell/UsageFooter.tsx: the 28px footer under a chat with
//! the provider usage chips (or the session chip, or Pi's chip), the
//! refresh button, and the running terminal control.
//!
//! The footer runs one usage operation at a time, as `inflight` did: a
//! refresh, a banked Codex reset, or a provider sign-in waits for the one
//! before it.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{LocalBoxFuture, Shared, join_all};
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, Context, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _,
};
use monocode_core::{HarnessId, Platform};
use monocode_layout::terminal_tab::{RunningTerminal, running_terminal_chip_label};
use monocode_ui::widgets::{PopoverSide, popover_frame, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use super::host::{HostTask, UsageHost};
use super::model::{
    ACCOUNT_USAGE_CLOCK_MS, CodexRateLimitResetOutcome, DEFAULT_PROVIDER_ACCOUNT_ID,
    ProviderAccount, ProviderRateLimits, RateLimitProvider, RateLimitStatus, error_rate_limits,
    idle_rate_limits, unavailable_rate_limits,
};
use super::pi_usage::PiUsage;
use super::popover::{Side, anchored_to_trigger, dismiss_outside};
use super::sign_in_panel::{SignInState, sign_in_panel};
use super::style::{hover_halo, palette, spin_icon, text};
use super::usage_chip::{ChipActions, ChipProps, UsageProviderChip};
use crate::settings::controls::TriggerBounds;
use crate::settings::providers::harness_logo;

/// `CLOCK_MS`.
const CLOCK_MS: u64 = ACCOUNT_USAGE_CLOCK_MS as u64;

/// `UsageFooterSession`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageFooterSession {
    pub id: Option<String>,
    pub harness: HarnessId,
    pub model: Option<String>,
    pub auth_required: bool,
    pub provider_account_id: Option<String>,
}

impl UsageFooterSession {
    pub fn new(harness: HarnessId) -> Self {
        Self {
            id: None,
            harness,
            model: None,
            auth_required: false,
            provider_account_id: None,
        }
    }
}

/// The footer's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageFooterProps {
    pub providers: Vec<RateLimitProvider>,
    pub session: Option<UsageFooterSession>,
    pub project: Option<String>,
    pub terminals: Vec<RunningTerminal>,
    pub terminal_open: bool,
    pub project_terminal_active: bool,
    /// For the "New Terminal" shortcut label.
    pub platform: Platform,
}

impl Default for UsageFooterProps {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            session: None,
            project: None,
            terminals: Vec::new(),
            terminal_open: false,
            project_terminal_active: false,
            platform: Platform::current(),
        }
    }
}

type Callback<A> = Option<Rc<dyn Fn(A, &mut Window, &mut App)>>;

/// The footer's callbacks. A missing one hides its control.
#[derive(Clone, Default)]
pub struct UsageFooterCallbacks {
    pub on_toggle_terminal: Callback<String>,
    pub on_new_terminal: Callback<()>,
    pub on_show_terminal: Callback<()>,
    /// After the footer saved the project's account choice.
    pub on_select_account: Callback<(HarnessId, String)>,
    pub on_manage_accounts: Callback<HarnessId>,
}

type Inflight = Shared<LocalBoxFuture<'static, ()>>;

/// `SessionChip`'s state.
#[derive(Default)]
struct SessionChipState {
    key: String,
    open: bool,
    login: SignInState,
    error: Option<String>,
}

/// `UsageFooter`.
pub struct UsageFooter {
    host: Rc<dyn UsageHost>,
    props: UsageFooterProps,
    callbacks: UsageFooterCallbacks,
    now: i64,
    refreshing: bool,
    inflight: Option<Inflight>,
    load_key: Option<String>,
    chips: HashMap<RateLimitProvider, Entity<UsageProviderChip>>,
    pi: Option<(String, Entity<PiUsage>)>,
    session_chip: SessionChipState,
    session_trigger: TriggerBounds,
    session_focus: FocusHandle,
    session_popover_focus: FocusHandle,
    terminal_trigger: TriggerBounds,
    terminal_menu: bool,
    animate: bool,
    _clock: Task<()>,
    _observe: Option<Subscription>,
}

impl UsageFooter {
    pub fn new(
        host: Rc<dyn UsageHost>,
        props: UsageFooterProps,
        callbacks: UsageFooterCallbacks,
        cx: &mut Context<Self>,
    ) -> Self {
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(CLOCK_MS))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.now = this.host.now();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let weak = cx.entity().downgrade();
        let observe = host.observe(
            Box::new(move |cx| {
                weak.update(cx, |_, cx| cx.notify()).ok();
            }),
            cx,
        );
        Self {
            now: host.now(),
            host,
            props,
            callbacks,
            refreshing: false,
            inflight: None,
            load_key: None,
            chips: HashMap::new(),
            pi: None,
            session_chip: SessionChipState::default(),
            session_trigger: TriggerBounds::default(),
            session_focus: cx.focus_handle(),
            session_popover_focus: cx.focus_handle(),
            terminal_trigger: TriggerBounds::default(),
            terminal_menu: false,
            animate: true,
            _clock: clock,
            _observe: observe,
        }
    }

    pub fn set_props(&mut self, props: UsageFooterProps, cx: &mut Context<Self>) {
        self.props = props;
        cx.notify();
    }

    pub fn set_callbacks(&mut self, callbacks: UsageFooterCallbacks, cx: &mut Context<Self>) {
        self.callbacks = callbacks;
        cx.notify();
    }

    /// Turns popover animations off, for screenshots.
    pub fn set_animate(&mut self, animate: bool, cx: &mut Context<Self>) {
        self.animate = animate;
        for chip in self.chips.values() {
            chip.update(cx, |chip, _| chip.set_animate(animate));
        }
        if let Some((_, pi)) = &self.pi {
            pi.update(cx, |pi, cx| pi.set_animate(animate, cx));
        }
    }

    pub fn props(&self) -> &UsageFooterProps {
        &self.props
    }

    pub fn is_refreshing(&self) -> bool {
        self.refreshing
    }

    /// The chip for a provider, once it has rendered.
    pub fn chip(&self, provider: RateLimitProvider) -> Option<&Entity<UsageProviderChip>> {
        self.chips.get(&provider)
    }

    /// The Pi session's chip, once it has rendered.
    pub fn pi_usage(&self) -> Option<&Entity<PiUsage>> {
        self.pi.as_ref().map(|(_, pi)| pi)
    }

    pub fn session_chip_open(&self) -> bool {
        self.session_chip.open
    }

    fn wants(&self, provider: RateLimitProvider) -> bool {
        self.props.providers.contains(&provider)
    }

    /// The account a provider's chip shows: the conversation's own, else the
    /// project's choice.
    fn account_id(&self, provider: RateLimitProvider, cx: &App) -> String {
        match &self.props.session {
            Some(session) if session.harness == provider.harness() => {
                match session.provider_account_id.as_deref() {
                    Some(id) if !id.is_empty() => return id.to_string(),
                    _ => {}
                }
            }
            _ => {}
        }
        self.host.selected_provider_account_id(
            provider.harness(),
            self.props.project.as_deref(),
            cx,
        )
    }

    fn account_available(&self, provider: RateLimitProvider, account_id: &str, cx: &App) -> bool {
        self.host
            .provider_accounts(provider.harness(), cx)
            .iter()
            .any(|account| account.id == account_id)
    }

    /// The snapshot a chip shows (`useCachedRateLimits`, or the removed
    /// account state).
    fn limits(&self, provider: RateLimitProvider, cx: &App) -> ProviderRateLimits {
        let account_id = self.account_id(provider, cx);
        if matches!(
            provider,
            RateLimitProvider::Claude | RateLimitProvider::Codex
        ) && !self.account_available(provider, &account_id, cx)
        {
            return unavailable_rate_limits(
                provider,
                "This conversation uses a removed account",
                self.now,
            );
        }
        self.host
            .rate_limits(provider, &account_id, cx)
            .unwrap_or_else(|| idle_rate_limits(provider))
    }

    /// The accounts the footer loads and refreshes, as `(provider, account)`.
    fn targets(&self, cx: &App) -> Vec<(RateLimitProvider, String)> {
        let mut targets = Vec::new();
        for provider in [RateLimitProvider::Claude, RateLimitProvider::Codex] {
            if !self.wants(provider) {
                continue;
            }
            let account_id = self.account_id(provider, cx);
            if self.account_available(provider, &account_id, cx) {
                targets.push((provider, account_id));
            }
        }
        for provider in [
            RateLimitProvider::Opencode,
            RateLimitProvider::Droid,
            RateLimitProvider::Grok,
        ] {
            if self.wants(provider) {
                targets.push((provider, DEFAULT_PROVIDER_ACCOUNT_ID.to_string()));
            }
        }
        targets
    }

    /// New accounts load once. Returning from Settings or focusing the
    /// window reads the shared snapshot without another request.
    fn sync_loads(&mut self, cx: &mut Context<Self>) {
        let targets = self.targets(cx);
        let key = targets
            .iter()
            .map(|(provider, account)| format!("{}:{account}", provider.as_str()))
            .collect::<Vec<_>>()
            .join("|");
        if self.load_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.load_key = Some(key);
        for (provider, account_id) in targets {
            self.host
                .load_rate_limits(provider, &account_id, false, cx)
                .detach();
        }
    }

    /// The refresh button. A refresh already running is shared.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.inflight.is_some() {
            return;
        }
        self.refreshing = true;
        let loads: Vec<Task<ProviderRateLimits>> = self
            .targets(cx)
            .into_iter()
            .map(|(provider, account_id)| {
                self.host.load_rate_limits(provider, &account_id, true, cx)
            })
            .collect();
        let (done, finished) = oneshot::channel::<()>();
        self.inflight = Some(finished.map(|_| ()).boxed_local().shared());
        cx.notify();
        cx.spawn(async move |this, cx| {
            join_all(loads).await;
            this.update(cx, |this, cx| {
                this.inflight = None;
                this.refreshing = false;
                cx.notify();
            })
            .ok();
            let _ = done.send(());
        })
        .detach();
    }

    /// Waits for any running operation, then claims the slot. Returns the
    /// sender that releases it.
    async fn claim(
        this: &gpui::WeakEntity<Self>,
        cx: &mut gpui::AsyncApp,
    ) -> Option<oneshot::Sender<()>> {
        loop {
            let claimed = this
                .update(cx, |this, cx| match this.inflight.clone() {
                    Some(running) => Err(running),
                    None => {
                        let (done, finished) = oneshot::channel::<()>();
                        this.inflight = Some(finished.map(|_| ()).boxed_local().shared());
                        this.refreshing = true;
                        cx.notify();
                        Ok(done)
                    }
                })
                .ok()?;
            match claimed {
                Ok(done) => return Some(done),
                Err(running) => running.await,
            }
        }
    }

    fn release(this: &gpui::WeakEntity<Self>, done: oneshot::Sender<()>, cx: &mut gpui::AsyncApp) {
        this.update(cx, |this, cx| {
            this.inflight = None;
            this.refreshing = false;
            cx.notify();
        })
        .ok();
        let _ = done.send(());
    }

    /// `consumeCodexReset`: spend a banked reset, then reload the account. A
    /// failure keeps the snapshot and shows the error.
    pub fn consume_codex_reset(
        &mut self,
        credit_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> HostTask<CodexRateLimitResetOutcome> {
        let account_id = self.account_id(RateLimitProvider::Codex, cx);
        let host = self.host.clone();
        cx.spawn(async move |this, cx| {
            let Some(done) = Self::claim(&this, cx).await else {
                return Err("Could not use Codex reset".into());
            };
            let consume = cx.update(|cx| {
                host.consume_codex_reset_credit(credit_id.as_deref(), &account_id, cx)
            });
            let result = match consume.await {
                Ok(outcome) => {
                    cx.update(|cx| {
                        host.load_rate_limits(RateLimitProvider::Codex, &account_id, true, cx)
                    })
                    .await;
                    Ok(outcome)
                }
                Err(error) => {
                    let message = if error.is_empty() {
                        "Could not use Codex reset".to_string()
                    } else {
                        error
                    };
                    cx.update(|cx| {
                        let previous = host.rate_limits(RateLimitProvider::Codex, &account_id, cx);
                        let value = error_rate_limits(
                            RateLimitProvider::Codex,
                            &message,
                            Some(
                                &previous
                                    .unwrap_or_else(|| idle_rate_limits(RateLimitProvider::Codex)),
                            ),
                            host.now(),
                        );
                        host.set_rate_limits(RateLimitProvider::Codex, &account_id, value, cx);
                    });
                    Err(message)
                }
            };
            Self::release(&this, done, cx);
            result
        })
    }

    /// `reconnectProvider`: sign in again, then check that usage loads.
    pub fn reconnect_provider(
        &mut self,
        provider: RateLimitProvider,
        account_id: String,
        cx: &mut Context<Self>,
    ) -> HostTask<()> {
        let host = self.host.clone();
        cx.spawn(async move |this, cx| {
            let Some(done) = Self::claim(&this, cx).await else {
                return Err("Could not complete sign-in".into());
            };
            let login = cx.update(|cx| {
                let account =
                    (account_id != DEFAULT_PROVIDER_ACCOUNT_ID).then_some(account_id.as_str());
                host.login_harness(provider.harness(), account, cx)
            });
            let mut result = login.await;
            if result.is_ok() {
                let value = cx
                    .update(|cx| host.load_rate_limits(provider, &account_id, true, cx))
                    .await;
                if value.status != RateLimitStatus::Ok {
                    result = Err(value
                        .error
                        .filter(|error| !error.is_empty())
                        .unwrap_or_else(|| {
                            format!(
                                "{} sign-in could not be verified",
                                provider.harness().title()
                            )
                        }));
                }
            }
            if let Err(error) = &result {
                let message = if error.is_empty() {
                    "Could not complete sign-in".to_string()
                } else {
                    error.clone()
                };
                cx.update(|cx| {
                    let previous = host.rate_limits(provider, &account_id, cx);
                    let value = error_rate_limits(
                        provider,
                        &message,
                        Some(&previous.unwrap_or_else(|| idle_rate_limits(provider))),
                        host.now(),
                    );
                    host.set_rate_limits(provider, &account_id, value, cx);
                });
                result = Err(message);
            }
            Self::release(&this, done, cx);
            result
        })
    }

    /// `selectAccount`: remember the project's choice, then tell the owner.
    pub fn select_account(
        &mut self,
        provider: HarnessId,
        account_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.host
            .select_provider_account(provider, self.props.project.as_deref(), &account_id, cx);
        if let Some(callback) = self.callbacks.on_select_account.clone() {
            callback((provider, account_id), window, cx);
        }
        cx.notify();
    }

    /// `addAccount`: a new profile, signed in, saved, and selected.
    pub fn add_account(
        &mut self,
        provider: HarnessId,
        label: String,
        cx: &mut Context<Self>,
    ) -> HostTask<ProviderAccount> {
        let host = self.host.clone();
        let account = match host.new_provider_account(provider, &label, cx) {
            Ok(account) => account,
            Err(error) => return Task::ready(Err(error)),
        };
        let login = host.login_harness(provider, Some(&account.id), cx);
        cx.spawn(async move |this, cx| {
            login.await?;
            cx.update(|cx| host.save_provider_account(&account, cx));
            let id = account.id.clone();
            let selected = this.update_in(cx, |this, window, cx| {
                this.select_account(provider, id.clone(), window, cx)
            });
            if selected.is_err() {
                // No window to hand the callback: still save the choice.
                this.update(cx, |this, cx| {
                    this.host.select_provider_account(
                        provider,
                        this.props.project.as_deref(),
                        &id,
                        cx,
                    );
                    cx.notify();
                })
                .ok();
            }
            Ok(account)
        })
    }

    fn chip_actions(&self, provider: RateLimitProvider, cx: &mut Context<Self>) -> ChipActions {
        let weak = cx.entity().downgrade();
        let mut actions = ChipActions::default();
        if matches!(
            provider,
            RateLimitProvider::Claude | RateLimitProvider::Codex
        ) {
            let harness = provider.harness();
            let select = weak.clone();
            actions.on_select_account = Some(Rc::new(move |id: String, window, cx| {
                select
                    .update(cx, |this, cx| this.select_account(harness, id, window, cx))
                    .ok();
            }));
            let add = weak.clone();
            actions.on_add_account = Some(Rc::new(move |label: String, _, cx| {
                add.update(cx, |this, cx| this.add_account(harness, label, cx))
                    .unwrap_or_else(|_| Task::ready(Err("Could not add this account".into())))
            }));
            if self.callbacks.on_manage_accounts.is_some() {
                let manage = self.callbacks.on_manage_accounts.clone();
                actions.on_manage_accounts = Some(Rc::new(move |(), window, cx| {
                    if let Some(manage) = &manage {
                        manage(harness, window, cx);
                    }
                }));
            }
        }
        if provider == RateLimitProvider::Codex {
            let consume = weak.clone();
            actions.on_consume_reset = Some(Rc::new(move |credit: Option<String>, _, cx| {
                consume
                    .update(cx, |this, cx| this.consume_codex_reset(credit, cx))
                    .unwrap_or_else(|_| Task::ready(Err("Could not use Codex reset".into())))
            }));
        }
        if matches!(
            provider,
            RateLimitProvider::Claude | RateLimitProvider::Codex | RateLimitProvider::Grok
        ) {
            let reconnect = weak;
            actions.on_reconnect = Some(Rc::new(move |(), _, cx| {
                reconnect
                    .update(cx, |this, cx| {
                        let account = if provider == RateLimitProvider::Grok {
                            DEFAULT_PROVIDER_ACCOUNT_ID.to_string()
                        } else {
                            this.account_id(provider, cx)
                        };
                        this.reconnect_provider(provider, account, cx)
                    })
                    .unwrap_or_else(|_| Task::ready(Err("Could not complete sign-in".into())))
            }));
        }
        actions
    }

    fn chip_props(&self, provider: RateLimitProvider, cx: &App) -> ChipProps {
        let mut props = ChipProps::new(self.limits(provider, cx), self.now);
        if provider != RateLimitProvider::Claude {
            props.project = self.props.project.clone();
        }
        if matches!(
            provider,
            RateLimitProvider::Claude | RateLimitProvider::Codex
        ) {
            props.accounts = self.host.provider_accounts(provider.harness(), cx);
            props.account_id = Some(self.account_id(provider, cx));
        }
        props
    }

    fn render_chip(
        &mut self,
        provider: RateLimitProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let props = self.chip_props(provider, cx);
        let actions = self.chip_actions(provider, cx);
        let host = self.host.clone();
        let animate = self.animate;
        let chip = self
            .chips
            .entry(provider)
            .or_insert_with(|| {
                cx.new(|cx| {
                    let mut chip =
                        UsageProviderChip::new(host, props.clone(), actions.clone(), window, cx);
                    chip.set_animate(animate);
                    chip
                })
            })
            .clone();
        chip.update(cx, |chip, _| chip.set_props(props, actions));
        chip.into_any_element()
    }

    fn render_pi(
        &mut self,
        session: &UsageFooterSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = format!(
            "{}:{}",
            session.id.as_deref().unwrap_or(""),
            session.model.as_deref().unwrap_or("")
        );
        let now = self.now;
        let model = session.model.clone();
        let stale = self.pi.as_ref().is_none_or(|(current, _)| *current != key);
        if stale {
            let host = self.host.clone();
            let animate = self.animate;
            let pi = cx.new(|cx| {
                let mut pi = PiUsage::new(host, model, now, window, cx);
                pi.set_animate(animate, cx);
                pi
            });
            self.pi = Some((key, pi));
        }
        let (_, pi) = self.pi.as_ref().expect("set above");
        let pi = pi.clone();
        pi.update(cx, |pi, _| pi.set_now(now));
        pi.into_any_element()
    }

    fn render_refresh(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let ink = theme.content(0.40);
        let glyph = if self.refreshing {
            spin_icon("footer-refresh-spin", IconName::RefreshCw, 10., ink)
        } else {
            icon(IconName::RefreshCw)
                .size(u(10.))
                .text_color(ink)
                .into_any_element()
        };
        let hover_fill = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let refreshing = self.refreshing;
        div()
            .id("footer-refresh")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(18.))
            .rounded(u(theme.radius.sm))
            .text_color(ink)
            .tooltip(tooltip("Refresh usage"))
            .debug_selector(|| "button:Refresh usage".into())
            .child(glyph)
            .map(|el| {
                if refreshing {
                    el.opacity(0.5)
                } else {
                    el.hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(cx)))
                }
            })
            .into_any_element()
    }

    fn sync_session_chip(&mut self) {
        let Some(session) = &self.props.session else {
            return;
        };
        let key = session
            .id
            .clone()
            .unwrap_or_else(|| session.harness.as_str().to_string());
        if self.session_chip.key != key {
            self.session_chip = SessionChipState {
                key,
                ..Default::default()
            };
        }
        if !session.auth_required && self.session_chip.login == SignInState::Complete {
            self.session_chip.login = SignInState::Idle;
        }
    }

    /// `SessionChip`'s sign-in.
    pub fn session_sign_in(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.props.session.clone() else {
            return;
        };
        self.session_chip.login = SignInState::Running;
        self.session_chip.error = None;
        cx.notify();
        let login = self.host.login_harness(session.harness, None, cx);
        cx.spawn(async move |this, cx| {
            let result = login.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.session_chip.open = false;
                        this.session_chip.login = SignInState::Complete;
                    }
                    Err(error) => {
                        this.session_chip.error = Some(if error.is_empty() {
                            "Could not complete sign-in".into()
                        } else {
                            error
                        });
                        this.session_chip.login = SignInState::Error;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_session_chip(
        &mut self,
        session: &UsageFooterSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let title = session.harness.title();
        let auth_required =
            session.auth_required && self.session_chip.login != SignInState::Complete;
        let can_login = auth_required && self.host.supports_harness_login(session.harness);
        let logo = provider_logo(harness_logo(session.harness)).size(12.);
        if !can_login {
            return div()
                .id("session-chip")
                .flex()
                .min_w_0()
                .items_center()
                .gap(u(6.))
                .whitespace_nowrap()
                .tooltip(tooltip(title))
                .debug_selector(|| "session-chip".into())
                .child(logo)
                .child(text(session.harness.label()))
                .into_any_element();
        }
        let hover_fill = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let label = format!("{title} sign-in required");
        let selector = format!("button:{label}");
        let sign_in_ink = if theme.is_dark() {
            palette::amber_300()
        } else {
            palette::amber_600()
        };
        let trigger = div()
            .id("session-chip")
            .group("session-chip")
            .track_focus(&self.session_focus)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(6.))
            .h(u(20.))
            .whitespace_nowrap()
            .text_color(theme.content(0.55))
            .hover(move |s| s.text_color(hover_ink))
            .child(hover_halo(
                "session-chip",
                4.,
                0.,
                theme.radius.sm,
                hover_fill,
            ))
            .tooltip(tooltip(label))
            .debug_selector(move || selector)
            .on_click(cx.listener(|this, _, window, cx| {
                this.session_chip.open = !this.session_chip.open;
                if this.session_chip.open {
                    window.focus(&this.session_popover_focus, cx);
                }
                cx.notify();
            }))
            .child(self.session_trigger.probe())
            .child(logo)
            .child(text(session.harness.label()))
            .when(auth_required, |el| {
                el.child(text("sign in").text_px(10.).text_color(sign_in_ink))
            });
        let popover = self.session_chip.open.then(|| {
            let weak = cx.entity().downgrade();
            let sign_in = weak.clone();
            let panel = sign_in_panel(
                session.harness,
                self.session_chip.login,
                self.session_chip.error.as_deref(),
                Rc::new(move |_, _, cx| {
                    sign_in.update(cx, |this, cx| this.session_sign_in(cx)).ok();
                }),
                None,
                None,
                cx,
            );
            let dialog_selector = format!("dialog:{title} sign-in");
            let content = div()
                .track_focus(&self.session_popover_focus)
                .debug_selector(move || dialog_selector)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.session_chip.open = false;
                        window.focus(&this.session_focus, cx);
                        cx.notify();
                    }
                }))
                .child(
                    popover_frame("session-sign-in")
                        .side(PopoverSide::Top)
                        .width(300.)
                        .animate(self.animate)
                        .child(panel),
                );
            let content = dismiss_outside(
                &self.session_trigger,
                Rc::new(move |_, cx| {
                    weak.update(cx, |this, cx| {
                        this.session_chip.open = false;
                        cx.notify();
                    })
                    .ok();
                }),
                content,
            );
            anchored_to_trigger(Side::Top, false, 7., window, cx, content)
        });
        div()
            .relative()
            .flex()
            .flex_none()
            .child(trigger)
            .children(popover)
            .into_any_element()
    }

    fn toggle_terminal(&mut self, file_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_menu = false;
        if let Some(toggle) = self.callbacks.on_toggle_terminal.clone() {
            toggle(file_id, window, cx);
        }
        cx.notify();
    }

    fn render_running_terminals(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let terminals = self.props.terminals.clone();
        let panel_open = self.props.terminal_open;
        let many = terminals.len() > 1;
        let label = running_terminal_chip_label(&terminals);
        let title = terminals
            .iter()
            .map(|terminal| format!("\"{}\" in {}", terminal.process, terminal.label))
            .collect::<Vec<_>>()
            .join("\n");
        let aria = if terminals.len() == 1 {
            if panel_open {
                format!("Hide {}", terminals[0].process)
            } else {
                format!("Show {}", terminals[0].process)
            }
        } else if panel_open {
            "Hide running terminals".into()
        } else {
            format!("{} terminals are running processes", terminals.len())
        };
        let hover_fill = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let first = terminals.first().map(|terminal| terminal.id.clone());
        let selector = format!("button:{aria}");
        let trigger = div()
            .id("running-terminals")
            .group("running-terminals")
            .relative()
            .flex()
            .min_w_0()
            .max_w(u(248.))
            .items_center()
            .gap(u(6.))
            .whitespace_nowrap()
            .hover(move |s| s.text_color(hover_ink))
            .child(hover_halo(
                "running-terminals",
                4.,
                0.,
                theme.radius.sm,
                hover_fill,
            ))
            .tooltip(tooltip(title))
            .debug_selector(move || selector)
            .on_click(cx.listener(move |this, _, window, cx| {
                if panel_open || !many {
                    if let Some(target) = first.clone() {
                        this.toggle_terminal(target, window, cx);
                    }
                    return;
                }
                this.terminal_menu = !this.terminal_menu;
                cx.notify();
            }))
            .child(self.terminal_trigger.probe())
            .child(terminal_live_mark())
            .child(
                text(label)
                    .truncate()
                    .font_family(theme.fonts.mono.clone())
                    .text_px(10.)
                    .tabular(),
            );
        let menu = (self.terminal_menu && many && !panel_open).then(|| {
            let hover = theme.content(0.10);
            let rows = terminals.iter().map(|terminal| {
                let id = terminal.id.clone();
                let selector = format!("menuitem:{}", terminal.process);
                div()
                    .id(SharedString::from(format!("terminal-{}", terminal.id)))
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .h(u(28.))
                    .w_full()
                    .rounded(u(theme.radius.lg))
                    .px(u(8.))
                    .text_px(12.)
                    .leading(theme.leading.none)
                    .text_color(theme.colors.content)
                    .hover(move |s| s.bg(hover))
                    .debug_selector(move || selector)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_terminal(id.clone(), window, cx)
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child(terminal.process.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(u(112.))
                            .truncate()
                            .text_px(11.)
                            .text_color(theme.content(0.40))
                            .child(terminal.label.clone()),
                    )
            });
            let weak = cx.entity().downgrade();
            let content = dismiss_outside(
                &self.terminal_trigger,
                Rc::new(move |_, cx| {
                    weak.update(cx, |this, cx| {
                        this.terminal_menu = false;
                        cx.notify();
                    })
                    .ok();
                }),
                popover_frame("running-terminals-menu")
                    .side(PopoverSide::Top)
                    .animate(self.animate)
                    .child(
                        div()
                            .min_w(u(192.))
                            .p(u(4.))
                            .flex()
                            .flex_col()
                            .debug_selector(|| "menu:Running terminals".into())
                            .children(rows),
                    ),
            );
            anchored_to_trigger(Side::Top, true, 6., window, cx, content)
        });
        div()
            .relative()
            .flex()
            .min_w_0()
            .child(trigger)
            .children(menu)
            .into_any_element()
    }

    fn render_terminal_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let active = self.props.project_terminal_active;
        let label = if active {
            "Terminal".to_string()
        } else {
            format!("New Terminal ({}`)", self.props.platform.mod_label())
        };
        let callback = if active {
            self.callbacks
                .on_show_terminal
                .clone()
                .or_else(|| self.callbacks.on_new_terminal.clone())
        } else {
            self.callbacks
                .on_new_terminal
                .clone()
                .or_else(|| self.callbacks.on_show_terminal.clone())
        };
        let hover_fill = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let selector = format!("button:{label}");
        div()
            .id("footer-terminal")
            .flex()
            .flex_none()
            .items_center()
            .gap(u(6.))
            .h(u(20.))
            .whitespace_nowrap()
            .rounded(u(theme.radius.sm))
            .px(u(6.))
            .text_color(if active {
                theme.colors.accent
            } else {
                theme.content(0.40)
            })
            .map(|el| {
                if active {
                    el.hover(move |s| s.bg(hover_fill))
                } else {
                    el.hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                }
            })
            .tooltip(tooltip(label))
            .debug_selector(move || selector)
            .on_click(move |_, window, cx| {
                if let Some(callback) = &callback {
                    callback((), window, cx);
                }
            })
            .child(icon(IconName::Terminal).size(u(14.)))
            .child(text("Terminal"))
            .into_any_element()
    }
}

/// `TerminalLiveMark`: three amber bars that light up in turn.
fn terminal_live_mark() -> AnyElement {
    let bar = |index: usize| {
        div()
            .w(gpui::px(4.))
            .h(gpui::px(8.))
            .bg(palette::terminal_live())
            .with_animation(
                SharedString::from(format!("terminal-live-{index}")),
                Animation::new(Duration::from_millis(3200)).repeat(),
                move |el, t| {
                    let lit = t >= 0.25 * (index + 1) as f32;
                    el.opacity(if lit { 0.85 } else { 0.4 })
                },
            )
    };
    div()
        .flex()
        .flex_none()
        .items_end()
        .gap(gpui::px(2.))
        .h(gpui::px(10.))
        .child(bar(0))
        .child(bar(1))
        .child(bar(2))
        .into_any_element()
}

impl Render for UsageFooter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_loads(cx);
        self.sync_session_chip();
        let theme = Theme::of(cx).clone();
        let opencode_unavailable = self.wants(RateLimitProvider::Opencode)
            && self.limits(RateLimitProvider::Opencode, cx).status == RateLimitStatus::Unavailable;
        let show_opencode = self.wants(RateLimitProvider::Opencode) && !opencode_unavailable;
        let show_usage = self.wants(RateLimitProvider::Claude)
            || self.wants(RateLimitProvider::Codex)
            || show_opencode
            || self.wants(RateLimitProvider::Droid)
            || self.wants(RateLimitProvider::Grok);
        let show_terminals = !self.props.terminals.is_empty();
        let show_terminal_button =
            self.callbacks.on_new_terminal.is_some() || self.callbacks.on_show_terminal.is_some();
        let session = self.props.session.clone();
        let pi_session = session
            .as_ref()
            .filter(|session| session.harness == HarnessId::Pi)
            .cloned();
        let label = if show_usage || pi_session.is_some() {
            Some("Provider usage")
        } else if show_terminals || show_terminal_button {
            Some("Terminals")
        } else if session.is_some() {
            Some("Session")
        } else {
            None
        };
        if pi_session.is_none() {
            self.pi = None;
        }
        let mut footer = div()
            .id("usage-footer")
            .flex()
            .flex_none()
            .items_center()
            .gap(u(6.))
            .h(u(theme.metrics.footer_height))
            .overflow_x_scroll()
            .border_t_1()
            .border_color(theme.colors.stroke)
            .px(u(12.))
            .text_px(theme.text.caption)
            .text_color(theme.content(0.55))
            .when_some(label, |el, label| {
                el.debug_selector(move || format!("footer:{label}"))
            });
        if let Some(session) = pi_session {
            footer = footer.child(self.render_pi(&session, window, cx));
        } else if show_usage {
            for provider in [RateLimitProvider::Claude, RateLimitProvider::Codex] {
                if self.wants(provider) {
                    footer = footer.child(self.render_chip(provider, window, cx));
                }
            }
            if show_opencode {
                footer = footer.child(self.render_chip(RateLimitProvider::Opencode, window, cx));
            }
            for provider in [RateLimitProvider::Droid, RateLimitProvider::Grok] {
                if self.wants(provider) {
                    footer = footer.child(self.render_chip(provider, window, cx));
                }
            }
            footer = footer.child(self.render_refresh(cx));
        } else if let Some(session) = &session {
            footer = footer.child(self.render_session_chip(session, window, cx));
        }
        if show_terminals || show_terminal_button {
            let control = if show_terminals {
                self.render_running_terminals(window, cx)
            } else {
                self.render_terminal_button(cx)
            };
            footer = footer.child(
                div()
                    .ml_auto()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(8.))
                    .child(control),
            );
        }
        footer
    }
}
