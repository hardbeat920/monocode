//! Durable completion groups for ordinary project sessions.
use super::*;
use monocode_core::store::{OperatorCompletionGroup, OperatorSubscription, WorkerOutcome};

impl Shell {
    pub(super) fn observe_operator_completions(
        &mut self,
        view: &Entity<Conversation>,
        caller: i64,
        cx: &mut Context<Self>,
    ) {
        let input = view.read(cx).input.clone();
        let mut empty = input.read(cx).content.is_empty();
        self.observations
            .push(cx.observe(&input, move |this, input, cx| {
                let was_empty = empty;
                empty = input.read(cx).content.is_empty();
                if empty && !was_empty && this.operator_subscriptions.contains_key(&caller) {
                    this.check_operator_completions(cx);
                }
            }));
    }
    pub(super) fn arm_operator_completion(
        &mut self,
        caller: i64,
        group: OperatorCompletionGroup,
        session: i64,
        title: String,
    ) -> Result<(), String> {
        let mut subscriptions = self
            .operator_subscriptions
            .get(&caller)
            .cloned()
            .unwrap_or_default();
        subscriptions.retain(|s| s.session != session && !s.delivered);
        subscriptions.push(OperatorSubscription {
            session,
            title,
            group,
            outcome: None,
            delivered: false,
        });
        self.save_operator_completions(caller, subscriptions)
    }
    pub(super) fn dismiss_operator_completion(
        &mut self,
        caller: i64,
        session: i64,
    ) -> Result<(), String> {
        self.operator_completion_storage_ready()?;
        let Some(mut subscriptions) = self.operator_subscriptions.get(&caller).cloned() else {
            return Ok(());
        };
        subscriptions.retain(|s| s.session != session);
        self.save_operator_completions(caller, subscriptions)
    }
    fn operator_completion_storage_ready(&self) -> Result<(), String> {
        match &self.operator_subscription_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
    fn save_operator_completions(
        &mut self,
        caller: i64,
        subscriptions: Vec<OperatorSubscription>,
    ) -> Result<(), String> {
        self.operator_completion_storage_ready()?;
        locked(&self.store)?.save_operator_subscriptions(caller, &subscriptions)?;
        if subscriptions.is_empty() {
            self.operator_subscriptions.remove(&caller);
        } else {
            self.operator_subscriptions.insert(caller, subscriptions);
        }
        Ok(())
    }
    pub(super) fn restore_operator_completions(&mut self, cx: &mut Context<Self>) {
        // An unfinished provider turn cannot survive an app restart.
        for subscriptions in self.operator_subscriptions.values_mut() {
            for subscription in subscriptions.iter_mut().filter(|s| !s.delivered) {
                subscription.outcome.get_or_insert(WorkerOutcome::Stopped);
            }
        }
        self.check_operator_completions(cx);
    }
    fn operator_group_ready(&self, caller: i64, group: &OperatorCompletionGroup, cx: &App) -> bool {
        let Some(subscriptions) = self.operator_subscriptions.get(&caller) else {
            return false;
        };
        let group: Vec<_> = subscriptions.iter().filter(|s| &s.group == group).collect();
        !group.is_empty()
            && group.iter().all(|s| {
                !s.delivered
                    && s.outcome.is_some()
                    && self.session_summaries.get(&s.session).is_none_or(|_| {
                        let metadata = self.session_metadata(s.session, cx);
                        !metadata.busy
                    })
            })
    }
    pub(super) fn check_operator_completions(&mut self, cx: &mut Context<Self>) {
        let callers: Vec<_> = self.operator_subscriptions.keys().copied().collect();
        for caller in callers {
            let mut subscriptions = self.operator_subscriptions[&caller].clone();
            if self
                .session_summaries
                .get(&caller)
                .is_none_or(|s| s.archived)
            {
                if let Err(error) = self.save_operator_completions(caller, Vec::new()) {
                    self.error = Some(error);
                }
                continue;
            }
            if self.deleting.contains(&caller) {
                continue;
            }
            let mut changed = false;
            for subscription in subscriptions.iter_mut().filter(|s| !s.delivered) {
                let outcome = if !self.session_summaries.contains_key(&subscription.session) {
                    Some(WorkerOutcome::Removed)
                } else if let Some(view) = self.sessions.get(&subscription.session) {
                    let view = view.read(cx);
                    if view.busy {
                        None
                    } else if subscription.outcome.is_none() {
                        view.worker_outcome()
                    } else {
                        subscription.outcome
                    }
                } else {
                    subscription.outcome
                };
                if subscription.outcome != outcome {
                    subscription.outcome = outcome;
                    changed = true;
                }
            }
            if changed && let Err(error) = self.save_operator_completions(caller, subscriptions) {
                self.error = Some(error);
                continue;
            }
            let groups: HashSet<_> = self.operator_subscriptions[&caller]
                .iter()
                .filter(|s| !s.delivered)
                .map(|s| s.group.clone())
                .collect();
            for group in groups {
                if self
                    .operator_completion_work
                    .contains(&(caller, group.clone()))
                    || self.archiving.contains(&caller)
                    || !self.operator_group_ready(caller, &group, cx)
                {
                    continue;
                }
                let Some(origin) = self.ensure_session(caller, cx) else {
                    continue;
                };
                origin.update(cx, |v, cx| v.load(cx));
                if !origin.read(cx).ready_for_coordinator_report(cx) {
                    continue;
                }
                let children: Vec<_> = self.operator_subscriptions[&caller]
                    .iter()
                    .filter(|s| s.group == group)
                    .cloned()
                    .collect();
                let responses: Vec<_> = children
                    .iter()
                    .map(|s| {
                        self.sessions
                            .get(&s.session)
                            .filter(|v| v.read(cx).history_loaded())
                            .map(|v| v.read(cx).external_outcome(cx).1)
                    })
                    .collect();
                self.operator_completion_work
                    .insert((caller, group.clone()));
                let store = self.store.clone();
                let work = cx.background_executor().spawn(async move {
                    children
                        .iter()
                        .zip(responses)
                        .map(|(s, response)| {
                            let prose = match response {
                                Some(response) => response,
                                None if s.outcome == Some(WorkerOutcome::Removed) => String::new(),
                                None => {
                                    let transcript = locked(&store)?.load(s.session)?;
                                    monocode_core::response::final_prose(&transcript.rows)
                                }
                            };
                            Ok(format!(
                                "Session {}: {}\nOutcome: {}\n{}",
                                s.session,
                                s.title,
                                s.outcome.unwrap().label(),
                                monocode_core::response::limit(&prose, 12_000)
                            ))
                        })
                        .collect::<Result<Vec<_>, String>>()
                        .map(|blocks| (children, blocks.join("\n\n")))
                });
                cx.spawn(async move |this, cx| {
                    let report = work.await;
                    let _ = this.update(cx, |this, cx| {
                        this.deliver_operator_completion(caller, group, report, cx);
                    });
                })
                .detach();
            }
        }
    }
    fn deliver_operator_completion(
        &mut self,
        caller: i64,
        group: OperatorCompletionGroup,
        report: Result<(Vec<OperatorSubscription>, String), String>,
        cx: &mut Context<Self>,
    ) {
        self.operator_completion_work
            .remove(&(caller, group.clone()));
        let (children, mut report) = match report {
            Ok(report) => report,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        if self
            .session_summaries
            .get(&caller)
            .is_none_or(|s| s.archived)
            || self.deleting.contains(&caller)
            || self.archiving.contains(&caller)
            || !self.operator_group_ready(caller, &group, cx)
        {
            return;
        }
        let current: Vec<_> = self.operator_subscriptions[&caller]
            .iter()
            .filter(|s| s.group == group)
            .map(|s| (s.session, s.outcome))
            .collect();
        if current
            != children
                .iter()
                .map(|s| (s.session, s.outcome))
                .collect::<Vec<_>>()
        {
            self.check_operator_completions(cx);
            return;
        }
        let Some(origin) = self.sessions.get(&caller).cloned() else {
            return;
        };
        if !origin.read(cx).ready_for_coordinator_report(cx) {
            return;
        }
        let mut subscriptions = self.operator_subscriptions[&caller].clone();
        for subscription in subscriptions.iter_mut().filter(|s| s.group == group) {
            subscription.delivered = true;
        }
        let pending: Vec<_> = subscriptions
            .iter()
            .filter(|s| !s.delivered)
            .map(|s| format!("Session {}: {}", s.session, s.title))
            .collect();
        if !pending.is_empty() {
            report.push_str(&format!(
                "\n\nOther reports pending:\n{}\nMore reports will follow.",
                pending.join("\n")
            ));
        }
        // Persist before dispatch for at-most-once delivery across a crash.
        if let Err(error) = self.save_operator_completions(caller, subscriptions) {
            self.error = Some(error);
            return;
        }
        origin.update(cx, |v, cx| v.send_coordinator_report(format!(
                            "[Delegated work completed]\nWorker completion is not verification: inspect the results, verify as needed, then tell the user.\n\n{report}"
                        ), cx));
    }
}
