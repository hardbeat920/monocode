//! The `UsageLimits` entity: turns stopped at a provider usage limit.
//!
//! Ports App.tsx lines 7536-7660: `onUsageLimitDismiss`,
//! `onUsageLimitResumeAtReset`, `onUsageLimitResume`, the reset timer that
//! sends the continue turn once an armed limit resets, and the lookup that
//! asks Claude or Codex when the stream did not say when the limit resets.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Context, Entity, Subscription, Task};
use monocode_core::HarnessId;
use monocode_core::session::UsageLimit;

use super::hooks::SubmitRequest;
use super::rate_limits::{RateLimitProvider, exhausted_window_reset_at};
use super::rate_limits_fetch::RateLimitFetcher;
use super::usage_limit::{USAGE_LIMIT_RESUME_GRACE_MS, usage_limit_resume_due};
use super::{Attention, Clock};
use crate::runtime::engine::Engine;
use crate::runtime::in_flight::CONTINUE_PROMPT;
use crate::runtime::sessions::Sessions;

/// Re-check every minute at most: timers drift while the machine sleeps.
const MAX_RECHECK_MS: i64 = 60_000;
const MIN_RECHECK_MS: i64 = 1_000;

/// A limit as a lookup key. The TypeScript kept looked-up limit objects in a
/// `WeakSet`; here a key lives while the session still holds that limit.
type LookupKey = (String, Option<i64>, Option<bool>);

fn lookup_key(session_id: &str, limit: &UsageLimit) -> LookupKey {
    (
        session_id.to_string(),
        limit.resets_at,
        limit.resume_at_reset,
    )
}

/// Usage limit timers and reset lookups.
pub struct UsageLimits {
    fetcher: Arc<dyn RateLimitFetcher>,
    clock: Clock,
    /// `usageResumingRef`: sessions with a resume scheduled.
    resuming: HashSet<String>,
    scheduled: Vec<Task<()>>,
    scheduled_ids: HashSet<String>,
    /// The `usageLimitTick` timer.
    recheck: Option<Task<()>>,
    /// `usageResetLookups`.
    lookups: HashSet<LookupKey>,
    _observe: Subscription,
}

impl UsageLimits {
    pub fn new(
        fetcher: Arc<dyn RateLimitFetcher>,
        clock: Clock,
        sessions: &Entity<Sessions>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut limits = Self {
            fetcher,
            clock,
            resuming: HashSet::new(),
            scheduled: Vec::new(),
            scheduled_ids: HashSet::new(),
            recheck: None,
            lookups: HashSet::new(),
            _observe: cx.observe(sessions, |this, _, cx| this.sessions_changed(cx)),
        };
        limits.sessions_changed(cx);
        limits
    }

    fn sessions_changed(&mut self, cx: &mut Context<Self>) {
        self.schedule_resumes(cx);
        self.look_up_resets(cx);
    }

    /// Sessions with a resume scheduled.
    pub fn resuming(&self) -> &HashSet<String> {
        &self.resuming
    }

    /// The resume effect: resume armed limits that are due on the next tick,
    /// and come back when the next one will be.
    fn schedule_resumes(&mut self, cx: &mut Context<Self>) {
        self.scheduled.clear();
        self.recheck = None;
        for id in self.scheduled_ids.drain() {
            self.resuming.remove(&id);
        }
        let now = (self.clock)();
        let mut next_check: Option<i64> = None;
        let mut due = Vec::new();
        for session in Engine::sessions(cx).read(cx).all() {
            let Some(limit) = session.usage_limit else {
                continue;
            };
            let (Some(true), Some(resets_at)) = (limit.resume_at_reset, limit.resets_at) else {
                continue;
            };
            if !usage_limit_resume_due(session, now) {
                let wait = resets_at + USAGE_LIMIT_RESUME_GRACE_MS - now;
                next_check = Some(next_check.map_or(wait, |next| next.min(wait)));
                continue;
            }
            if self.resuming.contains(&session.id) {
                continue;
            }
            due.push(session.id.clone());
        }
        for session_id in due {
            self.resuming.insert(session_id.clone());
            self.scheduled_ids.insert(session_id.clone());
            let clock = self.clock.clone();
            self.scheduled.push(cx.spawn(async move |this, cx| {
                let resume = this
                    .update(cx, |this, cx| {
                        this.resuming.remove(&session_id);
                        Engine::sessions(cx)
                            .read(cx)
                            .get(&session_id)
                            .is_some_and(|latest| usage_limit_resume_due(latest, clock()))
                    })
                    .unwrap_or(false);
                if resume {
                    cx.update(|cx| Self::resume(&session_id, cx));
                }
            }));
        }
        if let Some(next_check) = next_check {
            let wait = next_check.clamp(MIN_RECHECK_MS, MAX_RECHECK_MS);
            let timer = cx
                .background_executor()
                .timer(Duration::from_millis(wait as u64));
            self.recheck = Some(cx.spawn(async move |this, cx| {
                timer.await;
                this.update(cx, |this, cx| this.schedule_resumes(cx)).ok();
            }));
        }
    }

    /// The stream does not always say when the limit resets; ask the
    /// provider. Only Claude and Codex report it.
    fn look_up_resets(&mut self, cx: &mut Context<Self>) {
        let mut current = HashSet::new();
        let mut lookups = Vec::new();
        for session in Engine::sessions(cx).read(cx).all() {
            let Some(limit) = session.usage_limit else {
                continue;
            };
            let key = lookup_key(&session.id, &limit);
            current.insert(key.clone());
            if limit.resets_at.is_some() || self.lookups.contains(&key) {
                continue;
            }
            let provider = match session.harness {
                HarnessId::Claude => RateLimitProvider::Claude,
                HarnessId::Codex => RateLimitProvider::Codex,
                _ => continue,
            };
            self.lookups.insert(key);
            let account = session
                .provider_account_id
                .clone()
                .unwrap_or_else(|| "default".into());
            lookups.push((
                session.id.clone(),
                limit,
                self.fetcher.fetch(provider, &account),
            ));
        }
        // A limit that went away can come back as a new one.
        self.lookups.retain(|key| current.contains(key));
        for (session_id, limit, fetch) in lookups {
            cx.spawn(async move |_, cx| {
                let limits = fetch.await;
                let Some(resets_at) = exhausted_window_reset_at(&limits) else {
                    return;
                };
                cx.update(|cx| {
                    Engine::sessions(cx).update(cx, |sessions, cx| {
                        sessions.update_all(cx, |entry| {
                            (entry.id == session_id && entry.usage_limit == Some(limit)).then(
                                || {
                                    let mut next = entry.clone();
                                    next.usage_limit = Some(UsageLimit {
                                        resets_at: Some(resets_at),
                                        ..limit
                                    });
                                    next
                                },
                            )
                        });
                    });
                });
            })
            .detach();
        }
    }

    // The callbacks. They change `Sessions` and call the submit hook, so they
    // take the app rather than the entity.

    /// `onUsageLimitDismiss`.
    pub fn dismiss(session_id: &str, cx: &mut App) {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update_all(cx, |session| {
                (session.id == session_id && session.usage_limit.is_some()).then(|| {
                    let mut next = session.clone();
                    next.usage_limit = None;
                    next
                })
            });
        });
    }

    /// `onUsageLimitResumeAtReset`: arm or disarm the automatic resume.
    pub fn set_resume_at_reset(session_id: &str, enabled: bool, cx: &mut App) {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update_all(cx, |session| {
                let limit = session.usage_limit.filter(|_| session.id == session_id)?;
                let mut next = session.clone();
                next.usage_limit = Some(UsageLimit {
                    resume_at_reset: Some(enabled),
                    ..limit
                });
                Some(next)
            });
        });
    }

    /// `onUsageLimitResume`: clear the limit and send the continue turn.
    pub fn resume(session_id: &str, cx: &mut App) {
        let ready = Engine::sessions(cx)
            .read(cx)
            .get(session_id)
            .is_some_and(|session| session.usage_limit.is_some() && !session.is_busy());
        if !ready {
            return;
        }
        Self::dismiss(session_id, cx);
        Attention::submit(cx).submit(SubmitRequest::text(session_id, CONTINUE_PROMPT), cx);
    }
}
