//! Port of src/app/shell/UsageProviderChip.tsx: one provider's footer chip
//! and its popover, with the usage windows, the account switcher, adding an
//! account, the sign-in panel, and banked Codex resets with the mascot.
//!
//! The chip is an entity the footer owns. The footer passes fresh
//! [`ChipProps`] and [`ChipActions`] each render; the chip keeps the
//! popover's state, as the React component did.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable as _,
    Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, div,
    prelude::FluentBuilder as _, relative,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_core::HarnessId;
use monocode_layout::paths::{project_key, project_name};
use monocode_ui::widgets::{PopoverSide, popover_frame, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use super::account_usage::{
    MeterWidth, account_status_label, format_locale_date_time, meter_windows,
    provider_account_subtitle, usage_meter,
};
use super::host::{HostTask, UsageHost};
use super::mascot::banked_reset_mascot;
use super::model::{
    AccountStatusTone, CodexRateLimitResetOutcome, ProviderAccount, ProviderAccountIdentity,
    ProviderRateLimits, RateLimitProvider, RateLimitResetCredit, RateLimitStatus, RateLimitWindow,
    ResetCreditStatus, account_status, best_alternative_account, clamp_used_percent,
    format_rate_limit_window_chip_label, format_reset_countdown, format_reset_duration,
    format_usage_percent, format_window_label, identity_key, identity_organization_tag,
    rate_limit_window_tooltip, supports_provider_accounts,
};
use super::popover::{Side, anchored_to_trigger, dismiss_outside};
use super::sign_in_panel::{SignInState, sign_in_panel};
use super::style::{
    bar_color, css_percent, error_ink, hover_halo, parse_css_color, pulse, spin_icon, success_ink,
    text, warning_ink,
};
use crate::settings::controls::{TriggerBounds, plain_input};
use crate::settings::providers::harness_logo;

/// `presentation`: how a chip that is not a provider's own reads, as Pi's
/// chips do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presentation {
    pub harness: HarnessId,
    pub label: String,
    pub source_label: Option<String>,
}

/// The chip's data.
#[derive(Debug, Clone, PartialEq)]
pub struct ChipProps {
    pub limits: ProviderRateLimits,
    pub now: i64,
    pub presentation: Option<Presentation>,
    /// The project path, which picks the banked reset mascot.
    pub project: Option<String>,
    pub accounts: Vec<ProviderAccount>,
    pub account_id: Option<String>,
}

impl ChipProps {
    pub fn new(limits: ProviderRateLimits, now: i64) -> Self {
        Self {
            limits,
            now,
            presentation: None,
            project: None,
            accounts: Vec::new(),
            account_id: None,
        }
    }
}

type Action<A> = Rc<dyn Fn(A, &mut Window, &mut App)>;
type AsyncAction<A, T> = Rc<dyn Fn(A, &mut Window, &mut App) -> HostTask<T>>;

/// The chip's callbacks. A missing one hides its control, as an absent
/// React prop did.
#[derive(Clone, Default)]
pub struct ChipActions {
    pub on_select_account: Option<Action<String>>,
    /// Adds an account with this label and signs it in.
    pub on_add_account: Option<AsyncAction<String, ProviderAccount>>,
    pub on_manage_accounts: Option<Action<()>>,
    /// Spends a banked reset, by credit id when the row has one.
    pub on_consume_reset: Option<AsyncAction<Option<String>, CodexRateLimitResetOutcome>>,
    pub on_reconnect: Option<AsyncAction<(), ()>>,
}

/// Which page of the popover shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccountView {
    #[default]
    Usage,
    Accounts,
    Add,
}

/// `ResetActionState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResetAction {
    #[default]
    Idle,
    Confirming,
    Using,
    Outcome(CodexRateLimitResetOutcome),
    Error,
}

/// The windows a snapshot has, in order (`usageWindows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    Session,
    Weekly,
    Monthly,
}

/// `usageWindows`.
pub fn usage_windows(limits: &ProviderRateLimits) -> Vec<(WindowKind, RateLimitWindow)> {
    [
        (WindowKind::Session, limits.session),
        (WindowKind::Weekly, limits.weekly),
        (WindowKind::Monthly, limits.monthly),
    ]
    .into_iter()
    .filter_map(|(kind, window)| window.map(|window| (kind, window)))
    .collect()
}

/// `needsProviderLogin`: a missing or expired sign-in, not an account that
/// simply has no usage feed.
pub fn needs_provider_login(limits: &ProviderRateLimits) -> bool {
    if limits.status == RateLimitStatus::Unavailable {
        return true;
    }
    if limits.status != RateLimitStatus::Error {
        return false;
    }
    let text = limits.error.as_deref().unwrap_or("").to_lowercase();
    [
        "expired",
        "sign-in",
        "not signed in",
        "not connected",
        "authentication",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

/// `updatedLabel`.
pub fn updated_label(limits: &ProviderRateLimits, now: i64) -> String {
    if limits.updated_at <= 0 {
        return "Rate-limit details".into();
    }
    let elapsed = ((now - limits.updated_at) as f64 / 60_000.0)
        .floor()
        .max(0.0) as i64;
    if elapsed == 0 {
        return "Updated just now".into();
    }
    if elapsed < 60 {
        return format!("Updated {elapsed}m ago");
    }
    format!("Updated {}h ago", elapsed / 60)
}

/// `resetOutcomeLabel`.
pub fn reset_outcome_label(outcome: CodexRateLimitResetOutcome) -> &'static str {
    match outcome {
        CodexRateLimitResetOutcome::Reset => "Codex usage was reset.",
        CodexRateLimitResetOutcome::NothingToReset => "There’s no active usage to reset.",
        CodexRateLimitResetOutcome::NoCredit => "No banked resets are available.",
        CodexRateLimitResetOutcome::AlreadyRedeemed => "That reset was already used.",
    }
}

/// `emptyUsageLabel`.
pub fn empty_usage_label(limits: &ProviderRateLimits) -> &'static str {
    if limits.status != RateLimitStatus::Error {
        return "—";
    }
    let text = limits.error.as_deref().unwrap_or("").to_lowercase();
    if text.contains("expired") || text.contains("sign-in") {
        "expired"
    } else {
        "—"
    }
}

/// The card title for a window (`UsageWindowCard`).
pub fn window_title(kind: WindowKind) -> &'static str {
    match kind {
        WindowKind::Session => "5-hour limit",
        WindowKind::Weekly => "Weekly limit",
        WindowKind::Monthly => "Monthly limit",
    }
}

/// The banked reset rows: listed credits that can still be used, then one
/// placeholder per credit the backend only counted.
pub fn banked_reset_rows(limits: &ProviderRateLimits) -> Vec<Option<RateLimitResetCredit>> {
    let count = limits
        .reset_credits
        .as_ref()
        .map_or(0, |credits| credits.available_count);
    if count <= 0 {
        return Vec::new();
    }
    let detailed: Vec<RateLimitResetCredit> = limits
        .reset_credits
        .as_ref()
        .and_then(|credits| credits.credits.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|credit| {
            matches!(
                credit.status,
                ResetCreditStatus::Available | ResetCreditStatus::Unknown
            )
        })
        .collect();
    let unlisted = (count - detailed.len() as i64).max(0) as usize;
    detailed
        .into_iter()
        .map(Some)
        .chain(std::iter::repeat_n(None, unlisted))
        .collect()
}

/// A banked reset row's key: the credit id, or `unlisted-<index>`.
pub fn reset_row_key(credit: Option<&RateLimitResetCredit>, index: usize) -> String {
    credit.map_or_else(|| format!("unlisted-{index}"), |credit| credit.id.clone())
}

/// `UsageProviderChip`.
pub struct UsageProviderChip {
    host: Rc<dyn UsageHost>,
    props: ChipProps,
    actions: ChipActions,
    open: bool,
    /// Bumps on each open, so popover-scoped state (revealed emails, the
    /// open animation) starts fresh.
    opened: u64,
    account_view: AccountView,
    reset_action: ResetAction,
    active_reset_key: Option<String>,
    reset_error: Option<String>,
    reconnect_state: SignInState,
    reconnect_error: Option<String>,
    identities: HashMap<String, Option<ProviderAccountIdentity>>,
    identities_key: Option<String>,
    identities_task: Option<Task<()>>,
    account_usage_key: Option<String>,
    add_label: Entity<InputState>,
    add_running: bool,
    add_error: Option<String>,
    trigger: TriggerBounds,
    trigger_focus: FocusHandle,
    popover_focus: FocusHandle,
    animate: bool,
    _subscriptions: Vec<gpui::Subscription>,
}

impl UsageProviderChip {
    pub fn new(
        host: Rc<dyn UsageHost>,
        props: ChipProps,
        actions: ChipActions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let add_label = cx.new(|cx| InputState::new(window, cx).placeholder("Work or Personal"));
        let subscriptions = vec![cx.subscribe_in(
            &add_label,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit_add(window, cx),
                InputEvent::Change => {
                    // `maxLength={48}`.
                    let value = input.read(cx).value();
                    if value.chars().count() > 48 {
                        let clipped: String = value.chars().take(48).collect();
                        input.update(cx, |input, cx| input.set_value(clipped, window, cx));
                    }
                    cx.notify();
                }
                _ => {}
            },
        )];
        Self {
            host,
            props,
            actions,
            open: false,
            opened: 0,
            account_view: AccountView::Usage,
            reset_action: ResetAction::Idle,
            active_reset_key: None,
            reset_error: None,
            reconnect_state: SignInState::Idle,
            reconnect_error: None,
            identities: HashMap::new(),
            identities_key: None,
            identities_task: None,
            account_usage_key: None,
            add_label,
            add_running: false,
            add_error: None,
            trigger: TriggerBounds::default(),
            trigger_focus: cx.focus_handle(),
            popover_focus: cx.focus_handle(),
            animate: true,
            _subscriptions: subscriptions,
        }
    }

    /// New data and callbacks. The owner calls this before each render.
    pub fn set_props(&mut self, props: ChipProps, actions: ChipActions) {
        self.props = props;
        self.actions = actions;
    }

    pub fn props(&self) -> &ChipProps {
        &self.props
    }

    /// Turns the popover's open animations off, for screenshots.
    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn account_view(&self) -> AccountView {
        self.account_view
    }

    pub fn reset_action(&self) -> ResetAction {
        self.reset_action
    }

    pub fn reconnect_state(&self) -> SignInState {
        self.reconnect_state
    }

    /// The add account field.
    pub fn add_label_input(&self) -> &Entity<InputState> {
        &self.add_label
    }

    fn provider_label(&self) -> String {
        match &self.props.presentation {
            Some(presentation) => presentation.label.clone(),
            None => self.props.limits.provider.harness().title().to_string(),
        }
    }

    fn icon_harness(&self) -> HarnessId {
        self.props
            .presentation
            .as_ref()
            .map_or(self.props.limits.provider.harness(), |p| p.harness)
    }

    fn loading(&self) -> bool {
        let limits = &self.props.limits;
        limits.status == RateLimitStatus::Idle
            || (limits.status == RateLimitStatus::Fetching && !limits.has_window())
    }

    fn login_view(&self) -> bool {
        self.actions.on_reconnect.is_some()
            && !self.props.limits.has_window()
            && (needs_provider_login(&self.props.limits)
                || self.reconnect_state != SignInState::Idle)
    }

    fn can_manage_accounts(&self) -> bool {
        self.actions.on_select_account.is_some() && self.actions.on_add_account.is_some()
    }

    fn active_account(&self) -> Option<&ProviderAccount> {
        self.props
            .accounts
            .iter()
            .find(|account| Some(&account.id) == self.props.account_id.as_ref())
    }

    fn account_provider(&self) -> Option<RateLimitProvider> {
        let provider = self.props.limits.provider;
        supports_provider_accounts(provider.harness()).then_some(provider)
    }

    fn other_accounts(&self) -> Vec<ProviderAccount> {
        self.props
            .accounts
            .iter()
            .filter(|account| Some(&account.id) != self.props.account_id.as_ref())
            .cloned()
            .collect()
    }

    /// The footer's own snapshot is fresher for the active account.
    fn usage_for(&self, account: &ProviderAccount, cx: &App) -> Option<ProviderRateLimits> {
        if Some(&account.id) == self.props.account_id.as_ref() {
            return Some(self.props.limits.clone());
        }
        let provider = RateLimitProvider::from_harness(account.provider)?;
        self.host.rate_limits(provider, &account.id, cx)
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(cx);
        } else {
            self.open = true;
            self.opened += 1;
            window.focus(&self.popover_focus, cx);
        }
        cx.notify();
    }

    /// Closing resets every popover state (the `[open]` effect).
    fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.reset_action = ResetAction::Idle;
        self.active_reset_key = None;
        self.reset_error = None;
        self.reconnect_state = SignInState::Idle;
        self.reconnect_error = None;
        self.account_view = AccountView::Usage;
        self.add_running = false;
        self.add_error = None;
        cx.notify();
    }

    fn dismiss(&mut self, escape: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
        if escape {
            window.focus(&self.trigger_focus, cx);
        }
    }

    pub fn show_view(&mut self, view: AccountView, window: &mut Window, cx: &mut Context<Self>) {
        if view == AccountView::Add {
            self.add_running = false;
            self.add_error = None;
            self.add_label.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.set_disabled(false, cx);
            });
            let input = self.add_label.clone();
            input.update(cx, |input, cx| input.focus(window, cx));
        }
        self.account_view = view;
        cx.notify();
    }

    fn select_account(&mut self, account_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(select) = self.actions.on_select_account.clone() {
            select(account_id, window, cx);
        }
        self.close(cx);
    }

    fn manage_accounts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
        if let Some(manage) = self.actions.on_manage_accounts.clone() {
            manage((), window, cx);
        }
    }

    fn confirm_reset(&mut self, row_key: String, cx: &mut Context<Self>) {
        self.active_reset_key = Some(row_key);
        self.reset_action = ResetAction::Confirming;
        cx.notify();
    }

    fn cancel_reset(&mut self, cx: &mut Context<Self>) {
        self.active_reset_key = None;
        self.reset_action = ResetAction::Idle;
        cx.notify();
    }

    /// `useReset`.
    fn use_reset(
        &mut self,
        credit_id: Option<String>,
        row_key: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(consume) = self.actions.on_consume_reset.clone() else {
            return;
        };
        self.active_reset_key = Some(row_key);
        self.reset_action = ResetAction::Using;
        self.reset_error = None;
        cx.notify();
        let task = consume(credit_id, window, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(outcome) => this.reset_action = ResetAction::Outcome(outcome),
                    Err(error) => {
                        this.reset_error = Some(if error.is_empty() {
                            "Could not use this reset".into()
                        } else {
                            error
                        });
                        this.reset_action = ResetAction::Error;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `reconnect`.
    fn reconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(reconnect) = self.actions.on_reconnect.clone() else {
            return;
        };
        self.reconnect_state = SignInState::Running;
        self.reconnect_error = None;
        cx.notify();
        let task = reconnect((), window, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.reconnect_state = SignInState::Complete,
                    Err(error) => {
                        this.reconnect_error = Some(if error.is_empty() {
                            "Could not complete sign-in".into()
                        } else {
                            error
                        });
                        this.reconnect_state = SignInState::Error;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `AddProviderAccount`'s submit.
    pub fn submit_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let label = self.add_label.read(cx).value().to_string();
        let Some(add) = self.actions.on_add_account.clone() else {
            return;
        };
        if label.trim().is_empty() || self.add_running {
            return;
        }
        self.add_running = true;
        self.add_error = None;
        self.add_label
            .update(cx, |input, cx| input.set_disabled(true, cx));
        cx.notify();
        let task = add(label, window, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(_) => this.close(cx),
                    Err(error) => {
                        this.add_error = Some(if error.is_empty() {
                            "Could not add this account".into()
                        } else {
                            error
                        });
                        this.add_running = false;
                        this.add_label
                            .update(cx, |input, cx| input.set_disabled(false, cx));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `useProviderAccountIdentities(accounts, `${open}:${updatedAt}:${reconnectState}`)`.
    fn sync_identities(&mut self, cx: &mut Context<Self>) {
        let accounts = self.props.accounts.clone();
        let key = format!(
            "{}#{}:{}:{:?}",
            accounts
                .iter()
                .map(identity_key)
                .collect::<Vec<_>>()
                .join("|"),
            self.open,
            self.props.limits.updated_at,
            self.reconnect_state
        );
        if self.identities_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.identities_key = Some(key);
        if accounts.is_empty() {
            self.identities.clear();
            self.identities_task = None;
            return;
        }
        let load = self.host.account_identities(&accounts, cx);
        self.identities_task = Some(cx.spawn(async move |this, cx| {
            let identities = load.await;
            this.update(cx, |this, cx| {
                this.identities = identities;
                cx.notify();
            })
            .ok();
        }));
    }

    /// `useProviderAccountUsage`: while open, accounts without a snapshot
    /// load once.
    fn sync_account_usage(&mut self, cx: &mut Context<Self>) {
        let provider = self.account_provider();
        let enabled = self.open && provider.is_some() && !self.other_accounts().is_empty();
        let key = format!(
            "{}:{enabled}",
            self.props
                .accounts
                .iter()
                .map(|account| account.id.as_str())
                .collect::<Vec<_>>()
                .join("|")
        );
        if self.account_usage_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.account_usage_key = Some(key);
        let Some(provider) = provider.filter(|_| enabled) else {
            return;
        };
        for account in self.host.provider_accounts(provider.harness(), cx) {
            if self.host.rate_limits(provider, &account.id, cx).is_some() {
                continue;
            }
            self.host
                .load_rate_limits(provider, &account.id, false, cx)
                .detach();
        }
    }

    fn render_trigger(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let limits = &self.props.limits;
        let now = self.props.now;
        let windows = usage_windows(limits);
        let label = self.provider_label();
        let loading = self.loading();
        let disconnected = limits.status == RateLimitStatus::Unavailable;
        let tooltip_text = windows
            .iter()
            .map(|(_, window)| rate_limit_window_tooltip(window, now))
            .collect::<Vec<_>>()
            .join(" · ");
        let title = if !tooltip_text.is_empty() {
            tooltip_text
        } else if let Some(error) = limits.error.clone().filter(|error| !error.is_empty()) {
            error
        } else if disconnected {
            "Not connected".into()
        } else if loading {
            "Loading usage…".into()
        } else {
            "Usage details".into()
        };
        let muted = theme.content(0.35);
        let body: AnyElement = if loading {
            pulse("chip-loading", div().text_color(muted).child("···"))
        } else if disconnected {
            text("not connected").text_color(muted).into_any_element()
        } else if windows.is_empty() {
            text(empty_usage_label(limits))
                .text_color(muted)
                .into_any_element()
        } else {
            let tightest = windows.iter().map(|(_, window)| window.used_percent).fold(
                None::<f64>,
                |best, pct| match best {
                    Some(best) if pct <= best => Some(best),
                    _ => Some(pct),
                },
            );
            let account_label = (self.props.accounts.len() > 1)
                .then(|| self.active_account().map(|account| account.label.clone()))
                .flatten();
            let mut list = div().flex().min_w_0().items_center().gap(u(4.)).tabular();
            for (index, (_, window)) in windows.iter().enumerate() {
                if index > 0 {
                    list = list.child(div().text_color(theme.content(0.25)).child("·"));
                }
                list = list.child(text(format!(
                    "{} {}",
                    format_usage_percent(window.used_percent),
                    format_rate_limit_window_chip_label(window, now)
                )));
            }
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(u(6.))
                .when_some(account_label, |el, label| {
                    el.child(
                        text(label)
                            .max_w(u(96.))
                            .truncate()
                            .text_color(theme.content(0.45)),
                    )
                })
                .when_some(tightest, |el, pct| el.child(mini_bar(pct, cx)))
                .child(list)
                .into_any_element()
        };
        let hover_fill = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let selector = format!("button:{label} usage details");
        // `-mx-1 px-1`: the hover fill overhangs the content by 4px.
        div()
            .id("usage-chip-trigger")
            .group("usage-chip-trigger")
            .track_focus(&self.trigger_focus)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(6.))
            .h(u(20.))
            .whitespace_nowrap()
            .text_color(theme.content(0.55))
            .hover(move |s| s.text_color(hover_ink))
            .tooltip(tooltip(title))
            .debug_selector(move || selector)
            .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)))
            .child(hover_halo(
                "usage-chip-trigger",
                4.,
                0.,
                theme.radius.sm,
                hover_fill,
            ))
            .child(self.trigger.probe())
            .child(provider_logo(harness_logo(self.icon_harness())).size(12.))
            .child(body)
            .into_any_element()
    }

    fn render_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let label = self.provider_label();
        let login_view = self.login_view();
        let width = if self.account_view == AccountView::Accounts {
            340.
        } else {
            300.
        };
        let padded = !(self.account_view == AccountView::Usage && login_view);
        let body = match self.account_view {
            AccountView::Accounts => self.render_account_picker(&label, cx),
            AccountView::Add => self.render_add_account(&label, window, cx),
            AccountView::Usage if login_view => self.render_login(&label, cx),
            AccountView::Usage => self.render_usage(&label, cx),
        };
        let selector = format!("dialog:{label} usage details");
        let scroll = div()
            .id("usage-popover-scroll")
            .max_h(u(458.))
            .overflow_y_scroll()
            .when(padded, |el| el.p(u(10.)))
            .child(body);
        let frame = popover_frame(ElementId::NamedInteger("usage-popover".into(), self.opened))
            .side(PopoverSide::Top)
            .width(width)
            .max_height(460.)
            .animate(self.animate)
            .child(scroll);
        let this = cx.entity().downgrade();
        let content = div()
            .track_focus(&self.popover_focus)
            .debug_selector(move || selector)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.dismiss(true, window, cx);
                }
            }))
            .child(frame);
        let content = dismiss_outside(
            &self.trigger,
            Rc::new(move |window, cx| {
                this.update(cx, |this, cx| this.dismiss(false, window, cx))
                    .ok();
            }),
            content,
        );
        anchored_to_trigger(Side::Top, false, 7., window, cx, content)
    }

    fn render_usage(&mut self, label: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let limits = self.props.limits.clone();
        let now = self.props.now;
        let windows = usage_windows(&limits);
        let header = self.render_header(label, cx);
        let mut body = div().flex().flex_col().child(header);
        if limits.status == RateLimitStatus::Error && !windows.is_empty() {
            body = body.child(
                text("Couldn’t refresh. Showing the last available snapshot.")
                    .mb(u(8.))
                    .rounded(u(theme.radius.lg))
                    .bg(Hsla {
                        a: 0.1,
                        ..theme.colors.warning
                    })
                    .px(u(10.))
                    .py(u(8.))
                    .text_px(10.)
                    .line_height(u(16.))
                    .text_color(warning_ink(cx)),
            );
        }
        body = if windows.is_empty() {
            body.child(empty_usage_state(&limits, self.loading(), cx))
        } else {
            body.child(
                div().flex().flex_col().gap(u(6.)).children(
                    windows
                        .iter()
                        .map(|(kind, window)| usage_window_card(*kind, window, now, cx)),
                ),
            )
        };
        // `SwitchSuggestion`.
        let active_status = account_status(Some(&limits), now);
        if matches!(
            active_status.tone,
            AccountStatusTone::Exhausted | AccountStatusTone::Low
        ) && self.actions.on_select_account.is_some()
        {
            let others = self.other_accounts();
            let usage: Vec<(String, Option<ProviderRateLimits>)> = others
                .iter()
                .map(|account| (account.id.clone(), self.usage_for(account, cx)))
                .collect();
            let lookup = |account: &ProviderAccount| {
                usage
                    .iter()
                    .find(|(id, _)| *id == account.id)
                    .and_then(|(_, limits)| limits.clone())
            };
            if let Some(suggestion) = best_alternative_account(&others, lookup, now).cloned() {
                let suggestion_limits = lookup(&suggestion);
                body = body.child(self.render_switch_suggestion(
                    &suggestion,
                    suggestion_limits.as_ref(),
                    active_status.tone == AccountStatusTone::Exhausted,
                    cx,
                ));
            }
        }
        if limits.provider == RateLimitProvider::Codex {
            body = body.children(self.render_banked_resets(label, cx));
        }
        body.into_any_element()
    }

    fn render_header(&mut self, label: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let limits = &self.props.limits;
        let mut column = div()
            .flex()
            .flex_col()
            .min_w_0()
            .flex_1()
            .child(
                text(format!("{label} usage"))
                    .text_px(13.)
                    .medium()
                    .line_height(u(16.)),
            )
            .child(
                text(updated_label(limits, self.props.now))
                    .mt(u(2.))
                    .text_px(10.)
                    .line_height(u(16.))
                    .text_color(theme.content(0.40)),
            );
        if let Some(source) = self
            .props
            .presentation
            .as_ref()
            .and_then(|presentation| presentation.source_label.clone())
        {
            column = column.child(
                text(source)
                    .mt(u(2.))
                    .text_px(10.)
                    .line_height(u(16.))
                    .text_color(theme.content(0.55)),
            );
        }
        if self.can_manage_accounts() {
            column = column.child(self.render_account_line(label, cx));
        }
        let mut header = div()
            .flex()
            .items_start()
            .gap(u(10.))
            .px(u(4.))
            .pb(u(10.))
            .pt(u(2.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(u(28.))
                    .rounded(u(theme.radius.lg))
                    .bg(theme.content(0.06))
                    .border_1()
                    .border_color(theme.content(0.07))
                    .child(provider_logo(harness_logo(self.icon_harness())).size(16.)),
            )
            .child(column);
        if limits.status == RateLimitStatus::Fetching {
            let ink = theme.content(0.40);
            header = header.child(
                div()
                    .mt(u(2.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(4.))
                    .text_px(10.)
                    .text_color(ink)
                    .child(spin_icon("updating-spin", IconName::RefreshCw, 10., ink))
                    .child(text("Updating")),
            );
        }
        header.into_any_element()
    }

    /// The account line under the title: switching sits under the label,
    /// the email reveals on its own.
    fn render_account_line(&self, label: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let active = self.active_account().cloned();
        let account_label = active
            .as_ref()
            .map_or("Removed account".to_string(), |account| {
                account.label.clone()
            });
        let identity = active
            .as_ref()
            .and_then(|account| self.identities.get(&identity_key(account)).cloned())
            .flatten();
        let subtitle = provider_account_subtitle(
            ElementId::NamedInteger(
                SharedString::from(format!(
                    "active-subtitle-{}",
                    active.as_ref().map(identity_key).unwrap_or_default()
                )),
                self.opened,
            ),
            identity.as_ref(),
            None,
            theme.content(0.35),
            None,
        );
        let hover_fill = theme.content(0.10);
        let selector = format!("button:Switch {label} account");
        // `-ml-1 px-1`: the hover fill overhangs the text by 4px.
        div()
            .relative()
            .self_start()
            .mt(u(4.))
            .flex()
            .max_w_full()
            .items_center()
            .gap(u(4.))
            .py(u(2.))
            .text_px(10.)
            .text_color(theme.content(0.55))
            .child(
                div()
                    .id("switch-account")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(u(-4.))
                    .right(u(-4.))
                    .rounded(u(theme.radius.sm))
                    .hover(move |s| s.bg(hover_fill))
                    .debug_selector(move || selector)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_view(AccountView::Accounts, window, cx)
                    })),
            )
            .child(
                text(account_label)
                    .flex_none()
                    .max_w(relative(0.6))
                    .truncate(),
            )
            .children(subtitle)
            .child(icon(IconName::ChevronRight).size(u(10.)))
            .into_any_element()
    }

    fn render_switch_suggestion(
        &self,
        account: &ProviderAccount,
        limits: Option<&ProviderRateLimits>,
        exhausted: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let id = account.id.clone();
        let hover = theme.content(0.85);
        div()
            .mt(u(8.))
            .flex()
            .items_center()
            .gap(u(10.))
            .rounded(u(theme.radius.lg))
            .bg(theme.content(0.045))
            .border_1()
            .border_color(theme.content(0.06))
            .px(u(12.))
            .py(u(10.))
            .debug_selector(|| "switch-suggestion".into())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .flex_1()
                    .child(
                        text(format!(
                            "{} · switch to",
                            if exhausted {
                                "Out of usage"
                            } else {
                                "Running low"
                            }
                        ))
                        .text_px(10.)
                        .line_height(u(16.))
                        .text_color(theme.content(0.45)),
                    )
                    .child(
                        div()
                            .mt(u(2.))
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap(u(8.))
                            .text_px(11.)
                            .child(text(account.label.clone()).min_w_0().truncate().medium())
                            .child(account_status_label(
                                "suggestion-status",
                                &account_status(limits, self.props.now),
                                10.,
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .id("suggestion-switch")
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(u(28.))
                    .rounded(u(theme.radius.md))
                    .bg(theme.colors.content)
                    .px(u(10.))
                    .text_px(11.)
                    .medium()
                    .text_color(theme.colors.background_base)
                    .hover(move |s| s.bg(hover))
                    .debug_selector(|| "button:Switch".into())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_account(id.clone(), window, cx)
                    }))
                    .child("Switch"),
            )
            .into_any_element()
    }

    fn render_banked_resets(&self, label: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = Theme::of(cx).clone();
        let limits = &self.props.limits;
        let count = limits
            .reset_credits
            .as_ref()
            .map_or(0, |credits| credits.available_count);
        if count <= 0 {
            return None;
        }
        let rows = banked_reset_rows(limits);
        let (mascot_project, appearance_key) = match &self.props.project {
            Some(project) => (project_name(project), project_key(project)),
            None => (label.to_string(), label.to_string()),
        };
        let appearance = self
            .host
            .project_appearance(&appearance_key, &mascot_project, cx);
        let color = parse_css_color(&appearance.color).unwrap_or(theme.colors.accent);
        let can_use = self.actions.on_consume_reset.is_some();
        let card = div()
            .relative()
            .min_h(u(78.))
            .overflow_hidden()
            .rounded(u(theme.radius.lg))
            .bg(theme.content(0.04))
            .border_1()
            .border_color(theme.content(0.06))
            .px(u(12.))
            .py(u(12.))
            .pr(u(84.))
            .child(
                div()
                    .relative()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .child(text("Banked resets").text_px(11.).medium())
                            .child(
                                div()
                                    .rounded_full()
                                    .bg(theme.content(0.07))
                                    .border_1()
                                    .border_color(theme.content(0.07))
                                    .px(u(6.))
                                    .text_px(9.)
                                    .medium()
                                    .tabular()
                                    .text_color(theme.content(0.65))
                                    .child(count.to_string()),
                            ),
                    )
                    .child(
                        text(format!(
                            "{count} {} available",
                            if count == 1 { "reset" } else { "resets" }
                        ))
                        .mt(u(2.))
                        .text_px(10.)
                        .line_height(u(16.))
                        .text_color(theme.content(0.40)),
                    ),
            )
            .child(banked_reset_mascot(
                ElementId::NamedInteger("reset-mascot".into(), self.opened),
                &mascot_project,
                appearance.mascot.as_deref(),
                color,
                self.animate,
            ));
        let mut list = div().flex().flex_col().gap(u(6.));
        for (index, credit) in rows.iter().enumerate() {
            let row_key = reset_row_key(credit.as_ref(), index);
            let selected = self.active_reset_key.as_deref() == Some(row_key.as_str());
            let action = if selected {
                self.reset_action
            } else {
                ResetAction::Idle
            };
            let error = if selected {
                self.reset_error.clone()
            } else {
                None
            };
            let disabled = self.reset_action == ResetAction::Using && !selected;
            list = list.child(self.render_reset_row(
                credit.as_ref(),
                index,
                row_key,
                action,
                error,
                disabled,
                can_use,
                cx,
            ));
        }
        Some(
            div()
                .mt(u(10.))
                .border_t_1()
                .border_color(theme.content(0.08))
                .pt(u(10.))
                .child(card)
                .child(
                    div()
                        .id("banked-resets-list")
                        .mt(u(8.))
                        .max_h(u(224.))
                        .overflow_y_scroll()
                        .debug_selector(|| "list:Available banked resets".into())
                        .child(list),
                )
                .into_any_element(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_reset_row(
        &self,
        credit: Option<&RateLimitResetCredit>,
        index: usize,
        row_key: String,
        action: ResetAction,
        error: Option<String>,
        disabled: bool,
        can_use: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let now = self.props.now;
        let title = credit
            .and_then(|credit| credit.title.clone())
            .unwrap_or_else(|| format!("Banked reset {}", index + 1));
        let expiry = match credit.and_then(|credit| credit.expires_at) {
            None => "Expiry not provided".to_string(),
            Some(expires_at) if expires_at <= now => "Expires now".to_string(),
            Some(expires_at) => format!("Expires in {}", format_reset_duration(expires_at - now)),
        };
        let mut expiry_el = text(expiry)
            .id(SharedString::from(format!("expiry-{row_key}")))
            .min_w_0()
            .truncate()
            .text_px(10.)
            .tabular()
            .text_color(theme.content(0.40));
        if let Some(expires_at) = credit.and_then(|credit| credit.expires_at) {
            expiry_el = expiry_el.tooltip(tooltip(format_locale_date_time(expires_at)));
        }
        let trailing: Option<AnyElement> = match action {
            ResetAction::Using => {
                let ink = theme.content(0.45);
                Some(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(u(6.))
                        .text_px(10.)
                        .text_color(ink)
                        .child(spin_icon(
                            SharedString::from(format!("applying-{row_key}")),
                            IconName::RefreshCw,
                            12.,
                            ink,
                        ))
                        .child(text("Applying…"))
                        .into_any_element(),
                )
            }
            ResetAction::Outcome(_) | ResetAction::Error => {
                let message = match action {
                    ResetAction::Outcome(outcome) => reset_outcome_label(outcome).to_string(),
                    _ => error.unwrap_or_default(),
                };
                let ink = if action == ResetAction::Outcome(CodexRateLimitResetOutcome::Reset) {
                    success_ink(cx)
                } else {
                    theme.content(0.50)
                };
                Some(
                    text(message)
                        .flex_none()
                        .text_px(10.)
                        .text_color(ink)
                        .into_any_element(),
                )
            }
            _ if can_use && action != ResetAction::Confirming => {
                let key = row_key.clone();
                let selector = format!("button:Use reset:{row_key}");
                let hover_fill = theme.content(0.11);
                let hover_ink = theme.colors.content;
                let mut button = div()
                    .id(SharedString::from(format!("use-reset-{row_key}")))
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(u(24.))
                    .rounded(u(theme.radius.md))
                    .bg(theme.content(0.07))
                    .border_1()
                    .border_color(theme.content(0.08))
                    .px(u(10.))
                    .text_px(10.)
                    .medium()
                    .text_color(theme.content(0.70))
                    .debug_selector(move || selector)
                    .child("Use reset");
                if disabled {
                    button = button.opacity(0.35);
                } else {
                    button = button
                        .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.confirm_reset(key.clone(), cx)),
                        );
                }
                Some(button.into_any_element())
            }
            _ => None,
        };
        let mut row = div()
            .rounded(u(theme.radius.lg))
            .bg(theme.content(0.04))
            .border_1()
            .border_color(theme.content(0.06))
            .px(u(10.))
            .py(u(8.))
            .child(
                text(title)
                    .text_px(10.)
                    .medium()
                    .line_height(u(16.))
                    .text_color(theme.content(0.70)),
            )
            .when_some(
                credit.and_then(|credit| credit.description.clone()),
                |el, description| {
                    el.child(
                        text(description)
                            .mt(u(2.))
                            .text_px(10.)
                            .line_height(u(16.))
                            .text_color(theme.content(0.45)),
                    )
                },
            )
            .child(
                div()
                    .mt(u(6.))
                    .flex()
                    .min_h(u(24.))
                    .items_center()
                    .justify_between()
                    .gap(u(8.))
                    .child(expiry_el)
                    .children(trailing),
            );
        if action == ResetAction::Confirming {
            let credit_id = credit.map(|credit| credit.id.clone());
            let key = row_key.clone();
            let cancel_hover = theme.content(0.10);
            let cancel_hover_ink = theme.colors.content;
            row = row.child(
                div()
                    .mt(u(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(u(8.))
                    .border_t_1()
                    .border_color(theme.content(0.07))
                    .pt(u(8.))
                    .child(
                        text("Spend this reset now?")
                            .text_px(10.)
                            .line_height(u(16.))
                            .text_color(theme.content(0.50)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .gap(u(4.))
                            .child(
                                div()
                                    .id("reset-cancel")
                                    .flex()
                                    .items_center()
                                    .h(u(24.))
                                    .rounded(u(theme.radius.md))
                                    .px(u(8.))
                                    .text_px(10.)
                                    .text_color(theme.content(0.50))
                                    .hover(move |s| s.bg(cancel_hover).text_color(cancel_hover_ink))
                                    .debug_selector(|| "button:Cancel".into())
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel_reset(cx)))
                                    .child("Cancel"),
                            )
                            .child(
                                div()
                                    .id("reset-confirm")
                                    .flex()
                                    .items_center()
                                    .h(u(24.))
                                    .rounded(u(theme.radius.md))
                                    .bg(theme.colors.content)
                                    .px(u(10.))
                                    .text_px(10.)
                                    .medium()
                                    .text_color(theme.colors.background_base)
                                    .debug_selector(|| "button:Confirm".into())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.use_reset(credit_id.clone(), key.clone(), window, cx)
                                    }))
                                    .child("Confirm"),
                            ),
                    ),
            );
        }
        row.into_any_element()
    }

    fn render_login(&self, _label: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let mut column = div().flex().flex_col();
        if self.can_manage_accounts() {
            let account_label = self
                .active_account()
                .map_or("Removed account".to_string(), |account| {
                    account.label.clone()
                });
            let hover = theme.content(0.08);
            let selector = format!("button:Switch account from {account_label}");
            column = column.child(
                div().px(u(10.)).pt(u(10.)).child(
                    div()
                        .id("login-switch-account")
                        .flex()
                        .items_center()
                        .gap(u(8.))
                        .h(u(32.))
                        .w_full()
                        .rounded(u(theme.radius.lg))
                        .bg(theme.content(0.045))
                        .border_1()
                        .border_color(theme.content(0.06))
                        .px(u(10.))
                        .text_px(11.)
                        .hover(move |s| s.bg(hover))
                        .debug_selector(move || selector)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.show_view(AccountView::Accounts, window, cx)
                        }))
                        .child(text(account_label).min_w_0().flex_1().truncate())
                        .child(
                            div()
                                .text_px(10.)
                                .text_color(theme.content(0.40))
                                .child("Switch"),
                        )
                        .child(
                            icon(IconName::ChevronRight)
                                .size(u(12.))
                                .text_color(theme.content(0.35)),
                        ),
                ),
            );
        }
        let this = cx.entity().downgrade();
        column
            .child(sign_in_panel(
                self.props.limits.provider.harness(),
                self.reconnect_state,
                self.reconnect_error.as_deref(),
                Rc::new(move |_, window, cx| {
                    this.update(cx, |this, cx| this.reconnect(window, cx)).ok();
                }),
                None,
                None,
                cx,
            ))
            .into_any_element()
    }

    fn render_account_picker(&self, label: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let now = self.props.now;
        let mut rows = div().mt(u(8.)).flex().flex_col().gap(u(4.));
        for account in &self.props.accounts {
            let selected = Some(&account.id) == self.props.account_id.as_ref();
            let identity = self
                .identities
                .get(&identity_key(account))
                .cloned()
                .flatten();
            let org = identity_organization_tag(identity.as_ref());
            let usage = self.usage_for(account, cx);
            let meters = meter_windows(usage.as_ref());
            let status = account_status(usage.as_ref(), now);
            let id = account.id.clone();
            let row_id = SharedString::from(format!("account-{}", account.id));
            let button_selector = format!("button:{}", account.label);
            let hover = theme.content(0.04);
            let subtitle = provider_account_subtitle(
                ElementId::NamedInteger(
                    SharedString::from(format!("picker-subtitle-{}", identity_key(account))),
                    self.opened,
                ),
                identity.as_ref(),
                None,
                theme.content(0.35),
                Some(10.),
            );
            let status_el = div()
                .map(|el| {
                    if meters.is_empty() {
                        el.min_w_0()
                    } else {
                        el.flex_none()
                    }
                })
                .child(account_status_label(
                    SharedString::from(format!("status-{}", account.id)),
                    &status,
                    10.,
                    cx,
                ));
            let meters_el = (!meters.is_empty()).then(|| {
                div()
                    .flex()
                    .min_w_0()
                    .flex_1()
                    .gap(u(10.))
                    .children(meters.iter().map(|(title, window)| {
                        usage_meter(
                            SharedString::from(format!("meter-{}-{title}", account.id)),
                            &format_window_label(window.window_minutes),
                            window,
                            now,
                            MeterWidth::Flex,
                            cx,
                        )
                    }))
            });
            let row_selector = format!("account-row:{}", account.label);
            let mut row = div()
                .relative()
                .flex()
                .w_full()
                .items_center()
                .gap(u(12.))
                .rounded(u(theme.radius.lg))
                .px(u(10.))
                .py(u(8.))
                .text_px(11.)
                .border_1()
                .debug_selector(move || row_selector)
                .child(
                    div()
                        .id(row_id)
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .rounded(u(theme.radius.lg))
                        .hover(move |s| s.bg(hover))
                        .debug_selector(move || button_selector)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_account(id.clone(), window, cx)
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .flex_1()
                        .py(u(2.))
                        .child(
                            div()
                                .flex()
                                .min_w_0()
                                .items_baseline()
                                .gap(u(6.))
                                .child(text(account.label.clone()).flex_none().truncate())
                                .children(subtitle)
                                .when_some(org, |el, org| {
                                    el.child(
                                        text(org)
                                            .flex_none()
                                            .max_w(u(96.))
                                            .truncate()
                                            .rounded(u(theme.radius.sm))
                                            .bg(theme.content(0.07))
                                            .px(u(4.))
                                            .text_px(9.)
                                            .line_height(u(16.))
                                            .text_color(theme.content(0.50)),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .mt(u(4.))
                                .flex()
                                .min_w_0()
                                .items_center()
                                .gap(u(12.))
                                .text_px(10.)
                                .child(status_el)
                                .children(meters_el),
                        ),
                );
            row = if selected {
                row.bg(theme.accent(0.10))
                    .text_color(theme.colors.content)
                    .border_color(theme.accent(0.20))
                    .child(
                        icon(IconName::Check)
                            .size(u(14.))
                            .text_color(theme.colors.accent),
                    )
            } else {
                row.bg(theme.content(0.035))
                    .text_color(theme.content(0.70))
                    .border_color(theme.content(0.06))
            };
            rows = rows.child(row);
        }
        let hover_fill = theme.content(0.07);
        let hover_ink = theme.colors.content;
        let mut picker = div()
            .flex()
            .flex_col()
            .child(back_header(
                "Back to usage",
                format!("{label} accounts"),
                false,
                cx.listener(|this, _, window, cx| this.show_view(AccountView::Usage, window, cx)),
                cx,
            ))
            .child(
                text("Each conversation stays pinned to the account that started it.")
                    .mt(u(4.))
                    .px(u(4.))
                    .text_px(10.)
                    .line_height(u(16.))
                    .text_color(theme.content(0.40)),
            )
            .child(rows)
            .child(
                div()
                    .id("add-account")
                    .mt(u(8.))
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .h(u(32.))
                    .w_full()
                    .rounded(u(theme.radius.lg))
                    .px(u(10.))
                    .text_px(11.)
                    .text_color(theme.content(0.55))
                    .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                    .debug_selector(|| "button:Add account".into())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_view(AccountView::Add, window, cx)
                    }))
                    .child(icon(IconName::Plus).size(u(14.)))
                    .child("Add account"),
            );
        if self.actions.on_manage_accounts.is_some() {
            picker = picker.child(
                div()
                    .id("manage-accounts")
                    .mt(u(2.))
                    .flex()
                    .items_center()
                    .h(u(32.))
                    .w_full()
                    .rounded(u(theme.radius.lg))
                    .px(u(10.))
                    .text_px(11.)
                    .text_color(theme.content(0.45))
                    .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                    .debug_selector(|| "button:Manage accounts…".into())
                    .on_click(cx.listener(|this, _, window, cx| this.manage_accounts(window, cx)))
                    .child("Manage accounts…"),
            );
        }
        picker.into_any_element()
    }

    fn render_add_account(
        &self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let running = self.add_running;
        let value = self.add_label.read(cx).value().to_string();
        let disabled = running || value.trim().is_empty();
        let focused = self.add_label.read(cx).focus_handle(cx).is_focused(window);
        let ink = theme.colors.background_base;
        let submit_label = if running {
            "Waiting for browser…"
        } else {
            "Sign in and add account"
        };
        let hover = theme.content(0.85);
        let mut submit = div()
            .id("add-account-submit")
            .mt(u(12.))
            .flex()
            .items_center()
            .justify_center()
            .gap(u(6.))
            .h(u(32.))
            .w_full()
            .rounded(u(theme.radius.lg))
            .bg(theme.colors.content)
            .px(u(12.))
            .text_px(11.)
            .medium()
            .text_color(ink)
            .debug_selector(move || format!("button:{submit_label}"))
            .when(running, |el| {
                el.child(spin_icon("add-account-spin", IconName::RefreshCw, 14., ink))
            })
            .child(submit_label);
        if disabled {
            submit = submit.opacity(0.45);
        } else {
            submit = submit
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(|this, _, window, cx| this.submit_add(window, cx)));
        }
        let input = plain_input(&self.add_label, cx);
        div()
            .flex()
            .flex_col()
            .child(back_header(
                "Back to accounts",
                format!("Add {label} account"),
                running,
                cx.listener(|this, _, window, cx| {
                    this.show_view(AccountView::Accounts, window, cx)
                }),
                cx,
            ))
            .child(
                text("Give this account a local name, then finish sign-in in your browser.")
                    .mt(u(4.))
                    .px(u(4.))
                    .text_px(10.)
                    .line_height(u(16.))
                    .text_color(theme.content(0.40)),
            )
            .child(
                div()
                    .mt(u(12.))
                    .text_px(10.)
                    .medium()
                    .text_color(theme.content(0.55))
                    .child("Account name"),
            )
            .child(
                div()
                    .mt(u(6.))
                    .flex()
                    .items_center()
                    .h(u(32.))
                    .w_full()
                    .rounded(u(theme.radius.lg))
                    .border_1()
                    .border_color(if focused {
                        theme.accent(0.45)
                    } else {
                        theme.content(0.10)
                    })
                    .bg(theme.content(0.04))
                    .px(u(10.))
                    .text_px(11.)
                    .when(running, |el| el.opacity(0.55))
                    .debug_selector(|| "input:Account name".into())
                    .child(div().flex_1().min_w_0().child(input)),
            )
            .child(submit)
            .when_some(self.add_error.clone(), |el, error| {
                el.child(
                    text(error)
                        .mt(u(8.))
                        .text_px(10.)
                        .line_height(u(16.))
                        .text_color(error_ink(cx)),
                )
            })
            .into_any_element()
    }
}

impl Render for UsageProviderChip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_identities(cx);
        self.sync_account_usage(cx);
        let trigger = self.render_trigger(cx);
        let popover = self.open.then(|| self.render_popover(window, cx));
        div()
            .relative()
            .flex()
            .flex_none()
            .child(trigger)
            .children(popover)
    }
}

/// The back button and title the account pages start with.
fn back_header(
    action: &'static str,
    title: String,
    disabled: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let hover_fill = theme.content(0.10);
    let hover_ink = theme.colors.content;
    let mut back = div()
        .id(action)
        .flex()
        .items_center()
        .justify_center()
        .size(u(24.))
        .rounded(u(theme.radius.md))
        .text_color(theme.content(0.45))
        .debug_selector(move || format!("button:{action}"))
        .child(icon(IconName::ArrowLeft).size(u(14.)));
    if disabled {
        back = back.opacity(0.4);
    } else {
        back = back
            .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
            .on_click(on_click);
    }
    div()
        .flex()
        .items_center()
        .gap(u(4.))
        .h(u(28.))
        .child(back)
        .child(text(title).text_px(13.).medium())
        .into_any_element()
}

/// `MiniBar`: what remains of the tightest window.
fn mini_bar(used_pct: f64, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let pct = clamp_used_percent(used_pct);
    let remaining = 100.0 - pct;
    let selector = format!("minibar={}", css_percent(remaining));
    div()
        .flex_none()
        .h(u(4.))
        .w(u(32.))
        .overflow_hidden()
        .rounded_full()
        .bg(theme.content(0.10))
        .child(
            div()
                .h_full()
                .w(relative((remaining / 100.0) as f32))
                .rounded_full()
                .bg(bar_color(pct, cx))
                .debug_selector(move || selector),
        )
        .into_any_element()
}

/// `UsageWindowCard`.
fn usage_window_card(kind: WindowKind, window: &RateLimitWindow, now: i64, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let pct = clamp_used_percent(window.used_percent);
    let remaining = 100.0 - pct;
    let title = window_title(kind);
    let aria = format!("{title} remaining");
    let value_now = monocode_core::js::round(remaining) as i64;
    let bar_selector = format!("progressbar:{aria}={value_now}");
    let fill_selector = format!("fill:{aria}={}", css_percent(remaining));
    let reset = match window.resets_at {
        None => format!("{} window", format_window_label(window.window_minutes)),
        Some(resets_at) => format_reset_countdown(resets_at - now),
    };
    let mut reset_el = text(reset)
        .id(SharedString::from(format!("reset-{title}")))
        .truncate()
        .text_right()
        .tabular();
    if let Some(resets_at) = window.resets_at {
        reset_el = reset_el.tooltip(tooltip(format_locale_date_time(resets_at)));
    }
    div()
        .rounded(u(theme.radius.lg))
        .bg(theme.content(0.045))
        .border_1()
        .border_color(theme.content(0.06))
        .px(u(12.))
        .py(u(10.))
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .gap(u(12.))
                .child(
                    text(title)
                        .text_px(11.)
                        .medium()
                        .text_color(theme.content(0.65)),
                )
                .child(
                    text(format!("{} used", format_usage_percent(pct)))
                        .flex_none()
                        .text_px(11.)
                        .medium()
                        .tabular(),
                ),
        )
        .child(
            div()
                .mt(u(8.))
                .h(u(6.))
                .overflow_hidden()
                .rounded_full()
                .bg(theme.content(0.10))
                .debug_selector(move || bar_selector)
                .child(
                    div()
                        .h_full()
                        .w(relative((remaining / 100.0) as f32))
                        .rounded_full()
                        .bg(bar_color(pct, cx))
                        .debug_selector(move || fill_selector),
                ),
        )
        .child(
            div()
                .mt(u(6.))
                .flex()
                .items_center()
                .justify_between()
                .gap(u(12.))
                .text_px(10.)
                .line_height(u(16.))
                .text_color(theme.content(0.40))
                .child(text(format!("{value_now}% remaining")).tabular())
                .child(reset_el),
        )
        .into_any_element()
}

/// `EmptyUsageState`.
fn empty_usage_state(limits: &ProviderRateLimits, loading: bool, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let heading = if loading {
        "Loading usage…"
    } else if limits.status == RateLimitStatus::Unavailable {
        "Not connected"
    } else {
        "Usage unavailable"
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .rounded(u(theme.radius.lg))
        .bg(theme.content(0.04))
        .border_1()
        .border_color(theme.content(0.06))
        .px(u(12.))
        .py(u(16.))
        .child(
            text(heading)
                .text_px(11.)
                .medium()
                .text_color(theme.content(0.65)),
        )
        .when_some(
            limits.error.clone().filter(|error| !error.is_empty()),
            |el, error| {
                el.child(
                    text(error)
                        .mt(u(4.))
                        .max_w(u(240.))
                        .text_center()
                        .text_px(10.)
                        .line_height(u(16.))
                        .text_color(theme.content(0.40)),
                )
            },
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::model::{RateLimitResetCredits, ResetCreditType, idle_rate_limits};

    fn credit(id: &str, status: ResetCreditStatus) -> RateLimitResetCredit {
        RateLimitResetCredit {
            id: id.into(),
            reset_type: ResetCreditType::CodexRateLimits,
            status,
            granted_at: None,
            expires_at: None,
            title: None,
            description: None,
        }
    }

    #[test]
    fn does_not_describe_account_specific_usage_restrictions_as_login_failures() {
        let limits = ProviderRateLimits {
            error: Some("Claude usage is unavailable for this account".into()),
            status: RateLimitStatus::Error,
            ..idle_rate_limits(RateLimitProvider::Claude)
        };
        assert!(!needs_provider_login(&limits));
        let expired = ProviderRateLimits {
            error: Some("Claude sign-in expired".into()),
            ..limits.clone()
        };
        assert!(needs_provider_login(&expired));
        assert_eq!(empty_usage_label(&expired), "expired");
        assert_eq!(empty_usage_label(&limits), "—");
    }

    #[test]
    fn keeps_unlisted_resets_as_rows() {
        let limits = ProviderRateLimits {
            reset_credits: Some(RateLimitResetCredits {
                available_count: 3,
                credits: Some(vec![
                    credit("a", ResetCreditStatus::Available),
                    credit("b", ResetCreditStatus::Redeemed),
                ]),
            }),
            ..idle_rate_limits(RateLimitProvider::Codex)
        };
        let rows = banked_reset_rows(&limits);
        let keys: Vec<String> = rows
            .iter()
            .enumerate()
            .map(|(index, credit)| reset_row_key(credit.as_ref(), index))
            .collect();
        assert_eq!(keys, ["a", "unlisted-1", "unlisted-2"]);
    }

    #[test]
    fn labels_how_fresh_the_snapshot_is() {
        let limits = ProviderRateLimits {
            updated_at: 1_000_000,
            ..idle_rate_limits(RateLimitProvider::Claude)
        };
        assert_eq!(updated_label(&limits, 1_000_000), "Updated just now");
        assert_eq!(
            updated_label(&limits, 1_000_000 + 5 * 60_000),
            "Updated 5m ago"
        );
        assert_eq!(
            updated_label(&limits, 1_000_000 + 125 * 60_000),
            "Updated 2h ago"
        );
        assert_eq!(
            updated_label(&idle_rate_limits(RateLimitProvider::Claude), 0),
            "Rate-limit details"
        );
    }
}
