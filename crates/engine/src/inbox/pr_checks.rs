//! Port of src/features/inbox/hooks/useGithubPrChecks.ts as the `PrChecks`
//! entity.
//!
//! It loads a pull request's checks when created, whatever tab is showing,
//! keeps them fresh every 30 seconds for open PRs while polling is on and the
//! window is visible, and loads closed or merged PRs only at first and on
//! demand. Results replace each other whole, with their head commit, and a
//! late answer cannot outlive a PR change, a revision change, or the entity.
//! One request runs at a time; triggers that land meanwhile coalesce into one
//! follow-up, and a manual load outranks a queued poll.

use std::time::Duration;

use gpui::{Context, Task};

use super::client::InboxClient;
use super::types::GithubPrChecks;
use crate::runtime::Engine;

/// `POLL_MS`.
pub const PR_CHECKS_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// The hook's parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrChecksParams {
    pub cwd: String,
    pub repo: String,
    pub number: i64,
    pub enabled: bool,
    /// Open PRs poll; closed or merged ones load once and on demand.
    pub open: bool,
    /// The panel is showing. Defaults to true in the TypeScript.
    pub poll: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    Manual,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flight {
    ticket: u64,
    epoch: u64,
}

/// `GithubPrChecksView`.
pub struct PrChecks {
    client: InboxClient,
    params: PrChecksParams,
    checks: Option<GithubPrChecks>,
    loading: bool,
    refreshing: bool,
    error: Option<String>,
    stale: bool,
    has_data: bool,
    identity: Option<String>,
    epoch: u64,
    next_ticket: u64,
    flight: Option<Flight>,
    queued: Option<Trigger>,
    previous_poll: bool,
    timer: Option<Task<()>>,
}

impl PrChecks {
    pub fn new(client: InboxClient, params: PrChecksParams, cx: &mut Context<Self>) -> Self {
        let mut checks = Self {
            client,
            loading: params.enabled,
            previous_poll: params.poll,
            params,
            checks: None,
            refreshing: false,
            error: None,
            stale: false,
            has_data: false,
            identity: None,
            epoch: 0,
            next_ticket: 0,
            flight: None,
            queued: None,
            timer: None,
        };
        checks.load_effect(cx);
        checks.poll_effect(cx);
        checks
    }

    pub fn params(&self) -> &PrChecksParams {
        &self.params
    }

    /// The last answer: the head commit and its checks, together.
    pub fn checks(&self) -> Option<&GithubPrChecks> {
        self.checks.as_ref()
    }

    /// The first load: no results yet, so the view must not read as "no
    /// checks".
    pub fn loading(&self) -> bool {
        self.loading
    }

    /// Revalidating results already on screen.
    pub fn refreshing(&self) -> bool {
        self.refreshing
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Earlier results stayed on screen after a failed refresh.
    pub fn stale(&self) -> bool {
        self.stale
    }

    /// New parameters, as a re-render with new props.
    pub fn set_params(&mut self, params: PrChecksParams, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.params, params);
        let load_changed = old.cwd != self.params.cwd
            || old.repo != self.params.repo
            || old.number != self.params.number
            || old.enabled != self.params.enabled
            || old.revision != self.params.revision;
        let poll_changed = old.enabled != self.params.enabled
            || old.open != self.params.open
            || old.poll != self.params.poll;
        if load_changed {
            self.load_effect(cx);
        }
        if poll_changed {
            self.poll_effect(cx);
        }
    }

    /// The Refresh button.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load_effect(cx);
    }

    /// The window became visible again (`visibilitychange`).
    pub fn window_became_visible(&mut self, cx: &mut Context<Self>) {
        if self.timer.is_some() && !window_hidden(cx) {
            self.run(Some(Trigger::Auto), cx);
        }
    }

    /// The effect keyed on the PR, `enabled`, the revision, and manual loads.
    fn load_effect(&mut self, cx: &mut Context<Self>) {
        if !self.params.enabled {
            self.epoch += 1;
            self.flight = None;
            self.queued = None;
            self.identity = None;
            self.has_data = false;
            self.checks = None;
            self.loading = false;
            self.refreshing = false;
            self.error = None;
            self.stale = false;
            cx.notify();
            return;
        }
        let identity = format!(
            "{}\0{}\0{}",
            self.params.cwd, self.params.repo, self.params.number
        );
        if self.identity.as_deref() != Some(identity.as_str()) {
            // A different PR: drop the in-flight answer, any queue, and the
            // saved results.
            self.epoch += 1;
            self.identity = Some(identity);
            self.queued = None;
            self.has_data = false;
            self.checks = None;
            self.error = None;
            self.stale = false;
        }
        self.run(None, cx);
    }

    /// The effect keyed on `enabled`, `open`, and `poll`: the 30 second
    /// timer.
    fn poll_effect(&mut self, cx: &mut Context<Self>) {
        let resumed = self.params.poll && !self.previous_poll;
        self.previous_poll = self.params.poll;
        self.timer = None;
        if !self.params.enabled || !self.params.open || !self.params.poll {
            return;
        }
        if resumed && !window_hidden(cx) {
            self.run(Some(Trigger::Auto), cx);
        }
        self.timer = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(PR_CHECKS_POLL_INTERVAL)
                    .await;
                let alive = this
                    .update(cx, |this, cx| {
                        if !window_hidden(cx) {
                            this.run(Some(Trigger::Auto), cx);
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        }));
    }

    fn run(&mut self, trigger: Option<Trigger>, cx: &mut Context<Self>) {
        let automatic = trigger == Some(Trigger::Auto);
        if let Some(flight) = self.flight
            && flight.epoch == self.epoch
        {
            if !automatic || self.queued.is_none() {
                self.queued = Some(if automatic {
                    Trigger::Auto
                } else {
                    Trigger::Manual
                });
            }
            return;
        }
        self.queued = None;
        self.next_ticket += 1;
        let flight = Flight {
            ticket: self.next_ticket,
            epoch: self.epoch,
        };
        self.flight = Some(flight);
        if self.has_data {
            self.refreshing = true;
        } else {
            self.loading = true;
        }
        cx.notify();
        let pending = self.client.fetch_github_pr_checks(
            &self.params.cwd,
            &self.params.repo,
            self.params.number,
        );
        cx.spawn(async move |this, cx| {
            let result = pending.await;
            let _ = this.update(cx, |this, cx| this.settle(flight, result, cx));
        })
        .detach();
    }

    fn settle(
        &mut self,
        flight: Flight,
        result: Result<GithubPrChecks, String>,
        cx: &mut Context<Self>,
    ) {
        if flight.epoch == self.epoch {
            match result {
                Ok(next) => {
                    self.has_data = true;
                    // The head commit and its checks land together.
                    self.checks = Some(next);
                    self.error = None;
                    self.stale = false;
                }
                Err(error) => {
                    self.error = Some(error);
                    if self.has_data {
                        self.stale = true;
                    }
                }
            }
        }
        if self.flight.map(|current| current.ticket) != Some(flight.ticket) {
            cx.notify();
            return;
        }
        self.flight = None;
        if flight.epoch != self.epoch {
            cx.notify();
            return;
        }
        self.loading = false;
        self.refreshing = false;
        cx.notify();
        let Some(queued) = self.queued.take() else {
            return;
        };
        // An automatic follow-up waits for an open PR and a visible window;
        // the next tick or `window_became_visible` picks it back up.
        if queued == Trigger::Auto && (window_hidden(cx) || !self.params.open || !self.params.poll)
        {
            return;
        }
        self.run((queued == Trigger::Auto).then_some(Trigger::Auto), cx);
    }
}

fn window_hidden(cx: &gpui::App) -> bool {
    Engine::hooks(cx).workspace.window_hidden(cx)
}
