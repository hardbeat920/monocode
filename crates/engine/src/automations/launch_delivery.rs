//! Port of src/features/quick-composer/model/launchDelivery.ts: one
//! window's receiver for queued quick launches. Mount, focus, and launch
//! notices all drain the queue through one serialized loop, and only an
//! accepted launch is acknowledged. Receipts outlive a receiver, so a lost
//! acknowledgement never starts a second session. Transient failures retry
//! on a backoff timer even when no further notice arrives.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::Shared;
use gpui::{App, AsyncApp, Task};
use serde_json::Value;

use super::quick_composer::{QuickLaunchRequest, parse_quick_launch};

/// The first retry delay.
pub const INITIAL_RETRY_MS: u64 = 250;
/// The longest retry delay.
pub const MAX_RETRY_MS: u64 = 30_000;

/// Why a delivery stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// `InvalidLaunchError`: the queued launch is malformed. It stays queued
    /// without automatic retries.
    Invalid(String),
    /// `ProjectNotFoundError`: the project folder is gone. It stays queued
    /// until something outside retries, so timers do not keep adding error
    /// blocks.
    ProjectNotFound(String),
    /// Anything else; retried on the backoff timer.
    Failed(String),
}

impl LaunchError {
    pub fn message(&self) -> &str {
        match self {
            LaunchError::Invalid(message)
            | LaunchError::ProjectNotFound(message)
            | LaunchError::Failed(message) => message,
        }
    }

    /// Errors the receiver retries on its own.
    fn retries(&self) -> bool {
        matches!(self, LaunchError::Failed(_))
    }
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl From<String> for LaunchError {
    fn from(message: String) -> Self {
        LaunchError::Failed(message)
    }
}

/// A delivery attempt in flight, shared by everyone waiting on it.
pub type Delivery = Shared<Task<Result<(), LaunchError>>>;

/// `take`: the next queued envelope (`{ id, request }`) for this window.
pub type TakeFn = Rc<dyn Fn(&mut App) -> Task<Result<Option<Value>, LaunchError>>>;
/// `accept`: start the session. Resolves once the workspace accepted it.
pub type AcceptFn =
    Rc<dyn Fn(QuickLaunchRequest, String, &mut App) -> Task<Result<(), LaunchError>>>;
/// `ack`: remove the launch from the queue.
pub type AckFn = Rc<dyn Fn(String, &mut App) -> Task<Result<(), LaunchError>>>;

/// Acceptances by delivery id, shared across receivers of one window.
pub type Accepting = Rc<RefCell<HashMap<String, Delivery>>>;
/// Accepted delivery ids not yet acknowledged.
pub type Accepted = Rc<RefCell<HashSet<String>>>;

/// What `launchReceiver` takes.
#[derive(Clone)]
pub struct ReceiverOptions {
    pub take: TakeFn,
    pub accept: AcceptFn,
    pub ack: AckFn,
    /// The owner went away (the TypeScript effect cleanup).
    pub disposed: Rc<dyn Fn() -> bool>,
    pub accepted: Accepted,
    pub accepting: Accepting,
}

#[derive(Default)]
struct ReceiverState {
    running: Option<Delivery>,
    requested: bool,
    stopped: bool,
    retry: Option<Task<()>>,
    retry_delay_ms: u64,
}

/// `launchReceiver`.
#[derive(Clone)]
pub struct LaunchReceiver {
    state: Rc<RefCell<ReceiverState>>,
    options: Rc<ReceiverOptions>,
}

const INVALID: &str = "Invalid queued session; retained without automatic retry.";

impl LaunchReceiver {
    pub fn new(options: ReceiverOptions) -> Self {
        Self {
            state: Rc::new(RefCell::new(ReceiverState {
                retry_delay_ms: INITIAL_RETRY_MS,
                ..ReceiverState::default()
            })),
            options: Rc::new(options),
        }
    }

    fn disposed(&self) -> bool {
        self.state.borrow().stopped || (self.options.disposed)()
    }

    /// A retry timer is waiting.
    pub fn has_pending_retry(&self) -> bool {
        self.state.borrow().retry.is_some()
    }

    /// The current retry delay.
    pub fn retry_delay_ms(&self) -> u64 {
        self.state.borrow().retry_delay_ms
    }

    /// `receive`: drain this window's queue, joining a drain in flight.
    pub fn receive(&self, cx: &mut App) -> Delivery {
        if self.disposed() {
            return Task::ready(Ok(())).shared();
        }
        let mut state = self.state.borrow_mut();
        state.requested = true;
        if let Some(running) = &state.running {
            return running.clone();
        }
        // An outside notice retries now; do not leave a second timer alive.
        state.retry = None;
        let this = self.clone();
        let delivery = cx
            .spawn(async move |cx| {
                let result = this.drain(cx).await;
                this.finish(&result, cx);
                result
            })
            .shared();
        state.running = Some(delivery.clone());
        delivery
    }

    /// `dispose`: stop retrying.
    pub fn dispose(&self) {
        let mut state = self.state.borrow_mut();
        state.stopped = true;
        state.retry = None;
    }

    async fn drain(&self, cx: &mut AsyncApp) -> Result<(), LaunchError> {
        let options = self.options.clone();
        loop {
            self.state.borrow_mut().requested = false;
            while !self.disposed() {
                let take = cx.update(|cx| (options.take)(cx));
                let value = take.await?;
                if self.disposed() {
                    break;
                }
                let Some(value) = value.filter(|value| !value.is_null()) else {
                    self.state.borrow_mut().retry_delay_ms = INITIAL_RETRY_MS;
                    break;
                };
                let id = value
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string);
                let launch = parse_quick_launch(value.get("request").unwrap_or(&Value::Null));
                let (Some(id), Some(launch)) = (id, launch) else {
                    return Err(LaunchError::Invalid(INVALID.into()));
                };
                if !options.accepted.borrow().contains(&id) {
                    let acceptance = self.acceptance(&id, launch, cx);
                    acceptance.await?;
                }
                if self.disposed() {
                    break;
                }
                let ack = cx.update(|cx| (options.ack)(id.clone(), cx));
                ack.await?;
                options.accepted.borrow_mut().remove(&id);
                self.state.borrow_mut().retry_delay_ms = INITIAL_RETRY_MS;
            }
            let again = self.state.borrow().requested && !self.disposed();
            if !again {
                return Ok(());
            }
        }
    }

    /// The acceptance in flight for `id`, or a new one.
    fn acceptance(&self, id: &str, launch: QuickLaunchRequest, cx: &mut AsyncApp) -> Delivery {
        let options = self.options.clone();
        if let Some(existing) = options.accepting.borrow().get(id) {
            return existing.clone();
        }
        let id = id.to_string();
        let accept = cx.update(|cx| (options.accept)(launch, id.clone(), cx));
        let accepted = options.accepted.clone();
        let accepting = options.accepting.clone();
        let task_id = id.clone();
        let acceptance = cx
            .spawn(async move |_| {
                let result = accept.await;
                if result.is_ok() {
                    accepted.borrow_mut().insert(task_id.clone());
                }
                accepting.borrow_mut().remove(&task_id);
                result
            })
            .shared();
        options
            .accepting
            .borrow_mut()
            .insert(id, acceptance.clone());
        acceptance
    }

    /// The catch and finally of `receive`: schedule a retry for transient
    /// failures, then let the next notice start a new drain.
    fn finish(&self, result: &Result<(), LaunchError>, cx: &mut AsyncApp) {
        if let Err(error) = result
            && !self.disposed()
            && error.retries()
        {
            let delay = {
                let mut state = self.state.borrow_mut();
                let delay = state.retry_delay_ms;
                state.retry_delay_ms = (delay * 2).min(MAX_RETRY_MS);
                delay
            };
            let this = self.clone();
            let timer = cx.background_executor().timer(Duration::from_millis(delay));
            let retry = cx.spawn(async move |cx| {
                timer.await;
                this.state.borrow_mut().retry = None;
                // The retry schedules the next one on failure; nobody waits
                // on a timer attempt.
                cx.update(|cx| {
                    drop(this.receive(cx));
                });
            });
            self.state.borrow_mut().retry = Some(retry);
        }
        self.state.borrow_mut().running = None;
    }
}
