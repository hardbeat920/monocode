//! Test doubles for attention and for other packages' tests: a recording
//! platform, a scriptable rate limit fetcher, a recording submit hook and
//! approval router, a movable clock, and `init_test_attention`.
//!
//! Enable with the `test-support` feature from another crate.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use gpui::{App, TestAppContext};
use monocode_core::HarnessId;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::session::Session;
use monocode_core::user_question::UserQuestionReply;
use monocode_settings::Kv;
use parking_lot::Mutex;

use super::hooks::{ApprovalRouter, AttentionSubmit, SubmitRequest};
use super::notifications::{NotificationPermission, NotificationText};
use super::pi_usage::{PiUsageProvider, pi_billing_provider};
use super::platform::AttentionPlatform;
use super::rate_limits::{ProviderRateLimits, RateLimitProvider, idle_rate_limits};
use super::rate_limits_fetch::{CodexResetOutcome, RateLimitFetcher};
use super::sound_synth::SoundName;
use super::{Attention, AttentionConfig, Clock};
use crate::runtime::engine::Engine;
use crate::runtime::hooks::EngineHooks;
use crate::runtime::testing::{FakeBackend, TestWorkspace, init_test_engine_with};

/// A clock tests move by hand, alongside `advance_clock`.
#[derive(Clone, Default)]
pub struct TestClock(Arc<AtomicI64>);

impl TestClock {
    pub fn new(now: i64) -> Self {
        Self(Arc::new(AtomicI64::new(now)))
    }

    pub fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }

    pub fn set(&self, now: i64) {
        self.0.store(now, Ordering::SeqCst);
    }

    /// Move this clock and the test executor's timers together.
    pub fn advance(&self, cx: &mut TestAppContext, by: Duration) {
        self.0.fetch_add(by.as_millis() as i64, Ordering::SeqCst);
        cx.executor().advance_clock(by);
        cx.run_until_parked();
    }

    pub fn clock(&self) -> Clock {
        let now = self.0.clone();
        Arc::new(move || now.load(Ordering::SeqCst))
    }
}

/// A banner `show_notification` received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub session_id: String,
    pub text: NotificationText,
    pub sound: bool,
}

/// Records every OS call.
pub struct FakePlatform {
    pub permission: Mutex<NotificationPermission>,
    pub banners: Mutex<Vec<Banner>>,
    /// Make `show_notification` fail (`mockRejectedValue`).
    pub fail_banners: Mutex<bool>,
    pub badges: Mutex<Vec<u32>>,
    pub sounds: Mutex<Vec<SoundName>>,
    pub permission_requests: Mutex<usize>,
}

impl Default for FakePlatform {
    fn default() -> Self {
        Self {
            permission: Mutex::new(NotificationPermission::Granted),
            banners: Mutex::default(),
            fail_banners: Mutex::new(false),
            badges: Mutex::default(),
            sounds: Mutex::default(),
            permission_requests: Mutex::new(0),
        }
    }
}

impl FakePlatform {
    /// `(session id, body)` for every banner, like the TypeScript tests'
    /// `banners()`.
    pub fn banner_bodies(&self) -> Vec<(String, String)> {
        self.banners
            .lock()
            .iter()
            .map(|banner| (banner.session_id.clone(), banner.text.body.clone()))
            .collect()
    }
}

impl AttentionPlatform for FakePlatform {
    fn notification_permission(&self) -> NotificationPermission {
        *self.permission.lock()
    }

    fn request_notification_permission(&self) -> NotificationPermission {
        *self.permission_requests.lock() += 1;
        *self.permission.lock()
    }

    fn open_notification_settings(&self) -> Result<(), String> {
        Ok(())
    }

    fn show_notification(
        &self,
        session_id: &str,
        text: &NotificationText,
        sound: bool,
    ) -> Result<(), String> {
        if *self.fail_banners.lock() {
            return Err("No native bridge".into());
        }
        self.banners.lock().push(Banner {
            session_id: session_id.to_string(),
            text: text.clone(),
            sound,
        });
        Ok(())
    }

    fn set_dock_badge(&self, count: u32) {
        self.badges.lock().push(count);
    }

    fn play_sound(&self, sound: SoundName, _volume: f64) {
        self.sounds.lock().push(sound);
    }
}

type Gated = oneshot::Receiver<ProviderRateLimits>;

/// Scripted usage fetches. Each call takes the next queued answer for its
/// provider, or waits on a gate the test completes, or returns the
/// provider's default.
#[derive(Default)]
pub struct FakeFetcher {
    pub calls: Mutex<Vec<(RateLimitProvider, String)>>,
    pub pi_calls: Mutex<Vec<PiUsageProvider>>,
    answers: Mutex<HashMap<RateLimitProvider, VecDeque<ProviderRateLimits>>>,
    defaults: Mutex<HashMap<RateLimitProvider, ProviderRateLimits>>,
    gates: Mutex<HashMap<RateLimitProvider, VecDeque<Gated>>>,
    pi_answers: Mutex<HashMap<PiUsageProvider, ProviderRateLimits>>,
    pub reset_outcome: Mutex<Option<Result<CodexResetOutcome, String>>>,
}

impl FakeFetcher {
    /// The next call for `provider` returns `value`.
    pub fn push(&self, provider: RateLimitProvider, value: ProviderRateLimits) {
        self.answers
            .lock()
            .entry(provider)
            .or_default()
            .push_back(value);
    }

    /// Every call for `provider` without a queued answer returns `value`.
    pub fn set_default(&self, provider: RateLimitProvider, value: ProviderRateLimits) {
        self.defaults.lock().insert(provider, value);
    }

    /// The next call for `provider` waits until the sender completes.
    pub fn hold_next(&self, provider: RateLimitProvider) -> oneshot::Sender<ProviderRateLimits> {
        let (sender, receiver) = oneshot::channel();
        self.gates
            .lock()
            .entry(provider)
            .or_default()
            .push_back(receiver);
        sender
    }

    pub fn set_pi(&self, provider: PiUsageProvider, value: ProviderRateLimits) {
        self.pi_answers.lock().insert(provider, value);
    }

    pub fn call_count(&self, provider: RateLimitProvider) -> usize {
        self.calls
            .lock()
            .iter()
            .filter(|(called, _)| *called == provider)
            .count()
    }

    pub fn accounts_called(&self, provider: RateLimitProvider) -> Vec<String> {
        self.calls
            .lock()
            .iter()
            .filter(|(called, _)| *called == provider)
            .map(|(_, account)| account.clone())
            .collect()
    }
}

impl RateLimitFetcher for FakeFetcher {
    fn fetch(
        &self,
        provider: RateLimitProvider,
        account_id: &str,
    ) -> BoxFuture<'static, ProviderRateLimits> {
        self.calls.lock().push((provider, account_id.to_string()));
        if let Some(gate) = self
            .gates
            .lock()
            .get_mut(&provider)
            .and_then(VecDeque::pop_front)
        {
            return gate
                .map(move |value| value.unwrap_or_else(|_| idle_rate_limits(provider)))
                .boxed();
        }
        let value = self
            .answers
            .lock()
            .get_mut(&provider)
            .and_then(VecDeque::pop_front)
            .or_else(|| self.defaults.lock().get(&provider).cloned())
            .unwrap_or_else(|| idle_rate_limits(provider));
        futures::future::ready(value).boxed()
    }

    fn fetch_pi(&self, provider: PiUsageProvider) -> BoxFuture<'static, ProviderRateLimits> {
        self.pi_calls.lock().push(provider);
        let value = self
            .pi_answers
            .lock()
            .get(&provider)
            .cloned()
            .unwrap_or_else(|| idle_rate_limits(pi_billing_provider(provider)));
        futures::future::ready(value).boxed()
    }

    fn consume_codex_reset(
        &self,
        _credit_id: Option<String>,
        _account_id: &str,
    ) -> BoxFuture<'static, Result<CodexResetOutcome, String>> {
        let outcome = self
            .reset_outcome
            .lock()
            .clone()
            .unwrap_or(Ok(CodexResetOutcome::Reset));
        futures::future::ready(outcome).boxed()
    }
}

/// Records every `onSubmit`. With `start_turns` set it also does what the
/// submit pipeline does first: the session turns busy and a sent queued row
/// leaves the queue.
#[derive(Default)]
pub struct RecordingSubmit {
    pub requests: RefCell<Vec<SubmitRequest>>,
    pub start_turns: Cell<bool>,
}

impl AttentionSubmit for RecordingSubmit {
    fn submit(&self, request: SubmitRequest, cx: &mut App) {
        if self.start_turns.get() {
            let queued = request.queued_message_id.clone();
            Engine::sessions(cx).update(cx, |sessions, cx| {
                sessions.update(&request.session_id, cx, |session| {
                    session.busy = Some(true);
                    if let Some(queued) = &queued {
                        *session = super::queue::dequeue_queued_message(session, queued);
                    }
                });
            });
        }
        self.requests.borrow_mut().push(request);
    }
}

/// One routed answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Routed {
    Approval(HarnessId, String, i64, ApprovalDecision),
    Question(HarnessId, String, i64, UserQuestionReply),
    KeepOpen(HarnessId, String, i64),
    RemoteApproval(String, i64, ApprovalDecision),
    RemoteAnswer(String, i64, UserQuestionReply),
    InspectWorker(String),
    Focus(String),
    OpenHistory(String),
}

/// Records every routed answer. Tests set which sessions are remote, which
/// tabs are open, and the orchestrator's leads.
#[derive(Default)]
pub struct RecordingRouter {
    pub routed: RefCell<Vec<Routed>>,
    pub remote_sessions: RefCell<Vec<String>>,
    pub open_sessions: RefCell<Vec<String>>,
    pub leads: RefCell<HashMap<String, String>>,
}

impl ApprovalRouter for RecordingRouter {
    fn is_remote(&self, session: &Session, _cx: &App) -> bool {
        self.remote_sessions.borrow().contains(&session.id)
    }

    fn respond_approval(
        &self,
        harness: HarnessId,
        session_id: &str,
        request_id: i64,
        decision: ApprovalDecision,
        _cx: &mut App,
    ) {
        self.routed.borrow_mut().push(Routed::Approval(
            harness,
            session_id.into(),
            request_id,
            decision,
        ));
    }

    fn respond_question(
        &self,
        harness: HarnessId,
        session_id: &str,
        request_id: i64,
        reply: &UserQuestionReply,
        _cx: &mut App,
    ) {
        self.routed.borrow_mut().push(Routed::Question(
            harness,
            session_id.into(),
            request_id,
            reply.clone(),
        ));
    }

    fn keep_question_open(
        &self,
        harness: HarnessId,
        session_id: &str,
        request_id: i64,
        _cx: &mut App,
    ) {
        self.routed
            .borrow_mut()
            .push(Routed::KeepOpen(harness, session_id.into(), request_id));
    }

    fn remote_approve(
        &self,
        session_id: &str,
        request_id: i64,
        decision: ApprovalDecision,
        _cx: &mut App,
    ) {
        self.routed.borrow_mut().push(Routed::RemoteApproval(
            session_id.into(),
            request_id,
            decision,
        ));
    }

    fn remote_answer(
        &self,
        session_id: &str,
        request_id: i64,
        reply: &UserQuestionReply,
        _cx: &mut App,
    ) {
        self.routed.borrow_mut().push(Routed::RemoteAnswer(
            session_id.into(),
            request_id,
            reply.clone(),
        ));
    }

    fn orchestration_lead_for(&self, session_id: &str, _cx: &App) -> Option<String> {
        self.leads.borrow().get(session_id).cloned()
    }

    fn inspect_worker(&self, session_id: &str, _cx: &mut App) {
        self.routed
            .borrow_mut()
            .push(Routed::InspectWorker(session_id.into()));
    }

    fn focus_open_session(&self, session_id: &str, _cx: &mut App) -> bool {
        self.routed
            .borrow_mut()
            .push(Routed::Focus(session_id.into()));
        self.open_sessions
            .borrow()
            .iter()
            .any(|open| open == session_id)
    }

    fn open_history_session(&self, session_id: &str, _cx: &mut App) {
        self.routed
            .borrow_mut()
            .push(Routed::OpenHistory(session_id.into()));
    }
}

/// Everything `init_test_attention` installed.
pub struct TestAttention {
    pub backend: Arc<FakeBackend>,
    /// The engine's workspace hook. Sessions listed in `tab_session_ids`
    /// stay open; the rest detach when idle.
    pub workspace: Rc<TestWorkspace>,
    pub kv: Kv,
    pub platform: Arc<FakePlatform>,
    pub fetcher: Arc<FakeFetcher>,
    pub submit: Rc<RecordingSubmit>,
    pub router: Rc<RecordingRouter>,
    pub clock: TestClock,
}

/// The engine's `TestWorkspace`, as a global for test helpers.
pub struct TestWorkspaceGlobal(pub Rc<TestWorkspace>);

impl gpui::Global for TestWorkspaceGlobal {}

/// Install an `Engine` over a `FakeBackend` with a `TestWorkspace`.
pub fn init_test_engine_for_attention(
    cx: &mut TestAppContext,
) -> (Arc<FakeBackend>, Rc<TestWorkspace>) {
    let workspace = TestWorkspace::new();
    let hooks = EngineHooks {
        workspace: workspace.clone(),
        ..EngineHooks::default()
    };
    let backend = init_test_engine_with(cx, hooks);
    let global = TestWorkspaceGlobal(workspace.clone());
    cx.update(|cx| cx.set_global(global));
    (backend, workspace)
}

/// Install an `Engine` and `Attention` over fakes, with the clock at `now`.
pub fn init_test_attention(cx: &mut TestAppContext, now: i64) -> TestAttention {
    let (backend, workspace) = init_test_engine_for_attention(cx);
    init_test_attention_with(cx, backend, workspace, Kv::in_memory(), now)
}

/// `init_test_attention` over an engine already installed.
pub fn init_test_attention_with(
    cx: &mut TestAppContext,
    backend: Arc<FakeBackend>,
    workspace: Rc<TestWorkspace>,
    kv: Kv,
    now: i64,
) -> TestAttention {
    let platform = Arc::new(FakePlatform::default());
    let fetcher = Arc::new(FakeFetcher::default());
    let submit = Rc::new(RecordingSubmit::default());
    let router = Rc::new(RecordingRouter::default());
    let clock = TestClock::new(now);
    cx.update(|cx| {
        Attention::init(
            AttentionConfig {
                kv: kv.clone(),
                platform: platform.clone(),
                fetcher: fetcher.clone(),
                clock: clock.clock(),
            },
            cx,
        );
        Attention::set_submit(cx, submit.clone());
        Attention::set_approval_router(cx, router.clone());
    });
    cx.run_until_parked();
    TestAttention {
        backend,
        workspace,
        kv,
        platform,
        fetcher,
        submit,
        router,
        clock,
    }
}
