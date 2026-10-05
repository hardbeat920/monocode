//! GPUI behavior tests for the account views: clicks and keys in a test
//! window, ported from UsageProviderChip.test.ts, UsageFooter.test.ts,
//! UsageFooterAuth.test.ts, PiUsage.test.ts, HarnessUpdateNotice.test.ts,
//! NotificationMuteControl.test.ts, and ProjectNotificationSettings.test.ts.

mod chip;
mod footer;
mod notifications;
mod pi;
mod provider_accounts;
mod updates;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use futures::FutureExt as _;
use futures::channel::oneshot;
use gpui::{
    AnyView, App, Bounds, Context, Entity, IntoElement, Modifiers, ParentElement as _, Pixels,
    Render, Styled as _, Subscription, Task, TestAppContext, VisualTestContext, Window, div, px,
    size,
};
use monocode_core::HarnessId;
use monocode_ui::{AppearanceSettings, Theme};

use super::host::{HostTask, OnChange, ProjectAppearance, UsageHost};
use super::model::*;

pub(super) fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
    });
}

/// The window body: the view at the bottom, as the footer sits.
pub(super) struct Stage {
    view: AnyView,
}

impl Render for Stage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .child(self.view.clone())
    }
}

/// Opens a window with the view `build` returns.
pub(super) fn mount<V: Render>(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
) -> (Entity<V>, &'static mut VisualTestContext) {
    init(cx);
    let slot: Rc<RefCell<Option<Entity<V>>>> = Rc::new(RefCell::new(None));
    let out = slot.clone();
    let window = cx.open_window(size(px(width), px(height)), move |window, cx| {
        let view = build(window, cx);
        *out.borrow_mut() = Some(view.clone());
        Stage { view: view.into() }
    });
    let view = slot.borrow().clone().expect("view built");
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    draw(cx);
    (view, cx)
}

pub(super) fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.update(|window, cx| {
            window.draw(cx).clear();
        });
        cx.run_until_parked();
    }
}

fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

pub(super) fn bounds(cx: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    let selector = leak(selector.to_string());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("no element {selector}"))
}

pub(super) fn exists(cx: &mut VisualTestContext, selector: &str) -> bool {
    let selector = leak(selector.to_string());
    cx.debug_bounds(selector).is_some()
}

pub(super) fn click(cx: &mut VisualTestContext, selector: &str) {
    let at = bounds(cx, selector).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

/// Presses an element 3px in from its left edge, past anything drawn over
/// its middle.
pub(super) fn click_edge(cx: &mut VisualTestContext, selector: &str) {
    let bounds = bounds(cx, selector);
    let at = gpui::point(bounds.left() + px(3.), bounds.center().y);
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

/// Scrolls the scroll area under an element down by `dy` pixels.
pub(super) fn scroll(cx: &mut VisualTestContext, selector: &str, dy: f32) {
    let at = bounds(cx, selector).center();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: at,
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-dy))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    draw(cx);
}

pub(super) fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    draw(cx);
}

/// Types into a focused input.
pub(super) fn type_into(
    cx: &mut VisualTestContext,
    input: &Entity<gpui_component::input::InputState>,
    value: &str,
) {
    cx.update(|window, cx| input.update(cx, |input, cx| input.focus(window, cx)));
    cx.simulate_input(value);
    draw(cx);
}

/// A login the test finishes by hand.
pub(super) struct PendingLogin {
    pub harness: HarnessId,
    pub account_id: Option<String>,
    done: Option<oneshot::Sender<Result<(), String>>>,
}

type UsageAnswer = Box<dyn Fn(RateLimitProvider) -> ProviderRateLimits>;

/// A fake `UsageHost`: an in-memory cache with `loadRateLimits`'s
/// once-per-lifetime loads, scripted fetch answers, and logins the test
/// resolves.
pub(super) struct FakeUsage {
    pub now: Cell<i64>,
    pub visible: Cell<bool>,
    pub cache: RefCell<HashMap<String, ProviderRateLimits>>,
    /// `(provider, account)` for every fetch.
    pub fetches: RefCell<Vec<(RateLimitProvider, String)>>,
    /// Scripted fetch answers by provider; the default answer otherwise.
    pub answers: RefCell<HashMap<RateLimitProvider, VecDeque<ProviderRateLimits>>>,
    pub default_answer: RefCell<Option<UsageAnswer>>,
    pub logins: RefCell<Vec<PendingLogin>>,
    /// Logins that succeed at once.
    pub auto_login: Cell<bool>,
    pub login_support: RefCell<Vec<HarnessId>>,
    pub accounts: RefCell<HashMap<HarnessId, Vec<ProviderAccount>>>,
    pub selections: RefCell<HashMap<(HarnessId, String), String>>,
    pub selected: RefCell<Vec<(HarnessId, Option<String>, String)>>,
    pub saved: RefCell<Vec<ProviderAccount>>,
    pub identities: RefCell<HashMap<String, ProviderAccountIdentity>>,
    pub identity_reads: Cell<usize>,
    pub mascots: RefCell<HashMap<String, String>>,
    pub consumed: RefCell<Vec<Option<String>>>,
    pub consume_answer: RefCell<Result<CodexRateLimitResetOutcome, String>>,
    /// Pi fetches by provider, and scripted answers (a receiver keeps the
    /// fetch pending until the test sends).
    pub pi_fetches: RefCell<Vec<PiUsageProvider>>,
    pub pi_answers: RefCell<VecDeque<PiAnswer>>,
    pub pi_default: Cell<f64>,
    /// `useShowRemainingUsage` and `useMaskEmails`.
    pub show_remaining: Cell<bool>,
    pub mask_emails: Cell<bool>,
    observers: RefCell<Vec<Rc<OnChange>>>,
    next_account: Cell<usize>,
}

pub(super) enum PiAnswer {
    Now(ProviderRateLimits),
    Later(oneshot::Receiver<ProviderRateLimits>),
}

pub(super) const NOW: i64 = 1_789_560_000_000; // 2026-09-16T12:00:00Z

impl Default for FakeUsage {
    fn default() -> Self {
        Self {
            now: Cell::new(NOW),
            visible: Cell::new(true),
            cache: RefCell::default(),
            fetches: RefCell::default(),
            answers: RefCell::default(),
            default_answer: RefCell::new(None),
            logins: RefCell::default(),
            auto_login: Cell::new(false),
            login_support: RefCell::default(),
            accounts: RefCell::default(),
            selections: RefCell::default(),
            selected: RefCell::default(),
            saved: RefCell::default(),
            identities: RefCell::default(),
            identity_reads: Cell::new(0),
            mascots: RefCell::default(),
            consumed: RefCell::default(),
            consume_answer: RefCell::new(Ok(CodexRateLimitResetOutcome::Reset)),
            pi_fetches: RefCell::default(),
            pi_answers: RefCell::default(),
            pi_default: Cell::new(24.0),
            show_remaining: Cell::new(false),
            mask_emails: Cell::new(false),
            observers: RefCell::default(),
            next_account: Cell::new(1),
        }
    }
}

impl FakeUsage {
    fn key(provider: RateLimitProvider, account_id: &str) -> String {
        format!("{}:{account_id}", provider.as_str())
    }

    fn changed(&self, cx: &mut App) {
        let observers: Vec<Rc<OnChange>> = self.observers.borrow().clone();
        cx.defer(move |cx| {
            for observer in observers {
                observer(cx);
            }
        });
    }

    /// Saves `useShowRemainingUsage` the way any window would, telling the
    /// observers.
    pub fn set_show_remaining(&self, value: bool, cx: &mut App) {
        self.show_remaining.set(value);
        self.changed(cx);
    }

    /// Saves `useMaskEmails` the way any window would, telling the
    /// observers.
    pub fn set_mask_emails(&self, value: bool, cx: &mut App) {
        self.mask_emails.set(value);
        self.changed(cx);
    }

    /// Resolves the oldest pending login for `harness`.
    pub fn finish_login(&self, harness: HarnessId, result: Result<(), String>) {
        let mut logins = self.logins.borrow_mut();
        let login = logins
            .iter_mut()
            .find(|login| login.harness == harness && login.done.is_some())
            .expect("a pending login");
        let _ = login.done.take().expect("pending").send(result);
    }

    pub fn login_calls(&self) -> Vec<HarnessId> {
        self.logins
            .borrow()
            .iter()
            .map(|login| login.harness)
            .collect()
    }

    pub fn fetch_count(&self, provider: RateLimitProvider) -> usize {
        self.fetches
            .borrow()
            .iter()
            .filter(|(fetched, _)| *fetched == provider)
            .count()
    }

    pub fn set_accounts(&self, provider: HarnessId, accounts: Vec<ProviderAccount>) {
        self.accounts.borrow_mut().insert(provider, accounts);
    }

    pub fn answer(&self, provider: RateLimitProvider, value: ProviderRateLimits) {
        self.answers
            .borrow_mut()
            .entry(provider)
            .or_default()
            .push_back(value);
    }
}

impl UsageHost for FakeUsage {
    fn now(&self) -> i64 {
        self.now.get()
    }

    fn observe(&self, on_change: OnChange, _: &mut App) -> Option<Subscription> {
        self.observers.borrow_mut().push(Rc::new(on_change));
        None
    }

    fn rate_limits(
        &self,
        provider: RateLimitProvider,
        account_id: &str,
        _: &App,
    ) -> Option<ProviderRateLimits> {
        self.cache
            .borrow()
            .get(&Self::key(provider, account_id))
            .cloned()
    }

    fn load_rate_limits(
        &self,
        provider: RateLimitProvider,
        account_id: &str,
        force: bool,
        cx: &mut App,
    ) -> Task<ProviderRateLimits> {
        let key = Self::key(provider, account_id);
        if !force && let Some(cached) = self.cache.borrow().get(&key).cloned() {
            return Task::ready(cached);
        }
        self.fetches
            .borrow_mut()
            .push((provider, account_id.to_string()));
        let scripted = self
            .answers
            .borrow_mut()
            .get_mut(&provider)
            .and_then(VecDeque::pop_front);
        let value = scripted.unwrap_or_else(|| match self.default_answer.borrow().as_ref() {
            Some(answer) => answer(provider),
            None => ProviderRateLimits {
                status: RateLimitStatus::Ok,
                updated_at: self.now.get(),
                ..idle_rate_limits(provider)
            },
        });
        self.cache.borrow_mut().insert(key, value.clone());
        self.changed(cx);
        Task::ready(value)
    }

    fn set_rate_limits(
        &self,
        provider: RateLimitProvider,
        account_id: &str,
        value: ProviderRateLimits,
        cx: &mut App,
    ) {
        self.cache
            .borrow_mut()
            .insert(Self::key(provider, account_id), value);
        self.changed(cx);
    }

    fn consume_codex_reset_credit(
        &self,
        credit_id: Option<&str>,
        _: &str,
        _: &mut App,
    ) -> HostTask<CodexRateLimitResetOutcome> {
        self.consumed
            .borrow_mut()
            .push(credit_id.map(str::to_string));
        Task::ready(self.consume_answer.borrow().clone())
    }

    fn fetch_pi_usage(&self, provider: PiUsageProvider, cx: &mut App) -> Task<ProviderRateLimits> {
        self.pi_fetches.borrow_mut().push(provider);
        match self.pi_answers.borrow_mut().pop_front() {
            Some(PiAnswer::Now(value)) => Task::ready(value),
            Some(PiAnswer::Later(receiver)) => {
                let fallback = idle_rate_limits(pi_billing_provider(provider));
                cx.foreground_executor()
                    .spawn(receiver.map(move |value| value.unwrap_or(fallback)))
            }
            None => Task::ready(pi_quota(provider, self.pi_default.get())),
        }
    }

    fn window_visible(&self, _: &App) -> bool {
        self.visible.get()
    }

    fn show_remaining_usage(&self, _: &App) -> bool {
        self.show_remaining.get()
    }

    fn mask_emails(&self, _: &App) -> bool {
        self.mask_emails.get()
    }

    fn supports_harness_login(&self, harness: HarnessId) -> bool {
        self.login_support.borrow().contains(&harness)
    }

    fn login_harness(
        &self,
        harness: HarnessId,
        account_id: Option<&str>,
        cx: &mut App,
    ) -> HostTask<()> {
        let (done, result) = oneshot::channel();
        let auto = self.auto_login.get();
        self.logins.borrow_mut().push(PendingLogin {
            harness,
            account_id: account_id.map(str::to_string),
            done: if auto { None } else { Some(done) },
        });
        if auto {
            return Task::ready(Ok(()));
        }
        cx.foreground_executor()
            .spawn(result.map(|result| result.unwrap_or_else(|_| Err("Login cancelled".into()))))
    }

    fn provider_accounts(&self, provider: HarnessId, _: &App) -> Vec<ProviderAccount> {
        self.accounts
            .borrow()
            .get(&provider)
            .cloned()
            .unwrap_or_else(|| {
                vec![ProviderAccount {
                    is_default: Some(true),
                    ..ProviderAccount::new(DEFAULT_PROVIDER_ACCOUNT_ID, provider, "Default account")
                }]
            })
    }

    fn selected_provider_account_id(
        &self,
        provider: HarnessId,
        project: Option<&str>,
        _: &App,
    ) -> String {
        self.selections
            .borrow()
            .get(&(provider, project.unwrap_or("~").to_string()))
            .cloned()
            .unwrap_or_else(|| DEFAULT_PROVIDER_ACCOUNT_ID.into())
    }

    fn select_provider_account(
        &self,
        provider: HarnessId,
        project: Option<&str>,
        account_id: &str,
        cx: &mut App,
    ) {
        self.selected.borrow_mut().push((
            provider,
            project.map(str::to_string),
            account_id.to_string(),
        ));
        self.selections.borrow_mut().insert(
            (provider, project.unwrap_or("~").to_string()),
            account_id.to_string(),
        );
        self.changed(cx);
    }

    fn new_provider_account(
        &self,
        provider: HarnessId,
        label: &str,
        _: &mut App,
    ) -> Result<ProviderAccount, String> {
        let n = self.next_account.get();
        self.next_account.set(n + 1);
        Ok(ProviderAccount::new(
            &format!("account-{n}"),
            provider,
            label.trim(),
        ))
    }

    fn save_provider_account(&self, account: &ProviderAccount, cx: &mut App) {
        self.saved.borrow_mut().push(account.clone());
        let mut accounts = self.provider_accounts(account.provider, cx);
        accounts.push(account.clone());
        self.accounts
            .borrow_mut()
            .insert(account.provider, accounts);
        self.changed(cx);
    }

    fn account_identities(
        &self,
        accounts: &[ProviderAccount],
        _: &mut App,
    ) -> Task<HashMap<String, Option<ProviderAccountIdentity>>> {
        self.identity_reads.set(self.identity_reads.get() + 1);
        let identities = self.identities.borrow();
        Task::ready(
            accounts
                .iter()
                .map(|account| {
                    let key = identity_key(account);
                    let identity = identities.get(&key).cloned();
                    (key, identity)
                })
                .collect(),
        )
    }

    fn project_appearance(&self, project_key: &str, seed: &str, _: &App) -> ProjectAppearance {
        ProjectAppearance {
            mascot: self.mascots.borrow().get(project_key).cloned(),
            ..ProjectAppearance::hashed(seed)
        }
    }
}

/// `quota(percent)`: a Pi snapshot with one 5h window and no reset time.
pub(super) fn pi_quota(provider: PiUsageProvider, percent: f64) -> ProviderRateLimits {
    ProviderRateLimits {
        session: Some(RateLimitWindow {
            used_percent: percent,
            window_minutes: 300,
            resets_at: None,
        }),
        status: RateLimitStatus::Ok,
        updated_at: NOW,
        ..idle_rate_limits(pi_billing_provider(provider))
    }
}
