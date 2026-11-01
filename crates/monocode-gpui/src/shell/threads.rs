use super::*;
use monocode_core::{
    app_access as app,
    mono::bridge::Call,
    orchestration::{Summary, SummaryTask},
};
use serde_json::{Value, json};
use std::time::Duration;

impl Shell {
    pub(super) fn authorize_thread_caller(&self, call: &Call, cx: &App) -> Result<(), String> {
        let caller = self
            .sessions
            .get(&call.session)
            .ok_or("Caller is not available")?
            .read(cx);
        if self.thread_parents.contains_key(&call.session)
            || self.orchestration_worker(call.session)
            || caller.settings.mono.is_some()
            || self
                .monos
                .as_ref()
                .is_some_and(|m| m.read(cx).is_mono_session(call.session))
        {
            return Err("Only top-level threads can use thread tools".into());
        }
        if matches!(
            call.action.as_str(),
            "threads.start" | "threads.new" | "threads.send"
        ) && caller.thread_stopping()
        {
            return Err("Lead thread was stopped".into());
        }
        if let Some(id) = call.input["threadId"].as_i64()
            && self.thread_parents.get(&id) != Some(&call.session)
        {
            return Err("threadId must be one of your own children".into());
        }
        Ok(())
    }

    fn thread_status(&self, id: i64, cx: &App) -> Value {
        let metadata = self.session_metadata(id, cx);
        let view = self.sessions.get(&id).map(|v| v.read(cx));
        let waiting = view.and_then(|v| v.mobile_waiting(cx));
        let pending = view.is_some_and(|v| v.thread_pending());
        let outcome = if !pending && waiting.is_none() {
            view.filter(|v| v.history_loaded())
                .map(|v| v.thread_outcome(cx))
                .or_else(|| {
                    self.orchestration_summaries
                        .values()
                        .flat_map(|s| &s.tasks)
                        .find(|t| {
                            t.session == Some(id)
                                && matches!(t.status.as_str(), "completed" | "failed")
                        })
                        .map(|t| (t.status == "completed", String::new()))
                })
        } else {
            None
        };
        json!({"threadId":id,"title":metadata.summary.title,"harness":metadata.summary.provider,
            "model":view.and_then(|v| v.settings.model.clone()).or_else(||self.session_details.get(&id).and_then(|m|m.model.clone())),
            "status":if waiting.is_some(){"needsInput"}else if pending{"working"}else{"idle"},
            "pendingApproval":matches!(waiting,Some(crate::conversation::mobile::Waiting::Approval)),
            "pendingQuestion":matches!(waiting,Some(crate::conversation::mobile::Waiting::Question)),
            "success":outcome.as_ref().map(|(success,_)|success),"reply":outcome.as_ref().map(|(_,reply)|reply)})
    }

    pub(super) fn dispatch_thread_call(&mut self, call: Call, cx: &mut Context<Self>) {
        let id = call.input["threadId"].as_i64().unwrap_or_default();
        if call.action != "threads.list" {
            let Some(view) = self.ensure_session(id, cx) else {
                self.finish_app_call(call, Err("Child thread was not found".into()), cx);
                return;
            };
            view.update(cx, |v, cx| v.load(cx));
        }
        let result = match call.action.as_str() {
            "threads.list" => {
                let mut ids: Vec<_> = self
                    .thread_parents
                    .iter()
                    .filter(|(_, parent)| **parent == call.session)
                    .map(|(id, _)| *id)
                    .filter(|id| {
                        self.session_summaries.contains_key(id) && !self.thread_closed(*id, cx)
                    })
                    .collect();
                ids.sort_unstable();
                Ok(
                    json!({"threads":ids.into_iter().map(|id|self.thread_status(id,cx)).collect::<Vec<_>>()}),
                )
            }
            "threads.read" => {
                let status = self.thread_status(id, cx);
                let input = call.input.clone();
                let busy = status["status"] != "idle";
                self.app_background(
                    call,
                    move |store| {
                        let mut value =
                            locked(&store)?.app_conversation(id, &input, busy, false)?;
                        value["thread"] = status;
                        Ok((value, super::app_commands::Effect::None))
                    },
                    cx,
                );
                return;
            }
            "threads.wait" => {
                let status = self.thread_status(id, cx);
                let seconds = call.input["timeoutSeconds"].as_u64().unwrap_or(50);
                if status["status"] != "working" || seconds == 0 {
                    Ok(status)
                } else {
                    let session = call.session;
                    let request = call.request_id.clone();
                    self.thread_waiters.entry(id).or_default().push(call);
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(Duration::from_secs(seconds))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            let waiters = this.thread_waiters.entry(id).or_default();
                            if let Some(index) = waiters.iter().position(|call| {
                                call.session == session && call.request_id == request
                            }) {
                                let call = waiters.remove(index);
                                let status = this.thread_status(id, cx);
                                this.finish_app_call(call, Ok(status), cx);
                            }
                            if this.thread_waiters.get(&id).is_some_and(Vec::is_empty) {
                                this.thread_waiters.remove(&id);
                            }
                        });
                    })
                    .detach();
                    return;
                }
            }
            "threads.send" => {
                let view = self.sessions[&id].clone();
                if self.thread_closed(id, cx) {
                    Err("Child thread was closed".into())
                } else if !view.read(cx).ready_for_external(cx)
                    || view.read(cx).thread_pending()
                    || view.read(cx).has_saved_draft()
                {
                    Err("Child is busy, loading, or has an unsent draft".into())
                } else {
                    app::prompt(&call.input).map(|prompt| {
                        let sender = call.session;
                        view.update(cx, |v, cx| v.send_thread_message(prompt, sender, cx));
                        json!({"threadId":id,"accepted":true,"submitted":true})
                    })
                }
            }
            "threads.stop" => {
                self.sessions[&id].update(cx, |v, cx| v.stop_app_session(cx));
                self.release_thread_waiters(id, Err("Child thread was stopped".into()), cx);
                Ok(json!({"threadId":id,"stopped":true}))
            }
            "threads.close" => {
                if self.thread_status(id, cx)["status"] != "idle" {
                    Err("Child is still working or needs input; stop it first".into())
                } else {
                    self.close_threads(call.session, &[id], cx);
                    Ok(json!({"threadId":id,"closed":true}))
                }
            }
            _ => Err("Unknown thread action".into()),
        };
        self.finish_app_call(call, result, cx);
    }

    /// A closed child is archived: off its lead's list, rail and spawn tree.
    fn thread_closed(&self, id: i64, cx: &App) -> bool {
        (self.sessions.contains_key(&id) || self.session_summaries.contains_key(&id))
            && self.session_metadata(id, cx).summary.archived
    }

    /// Archives the settled children of `lead` among `ids`; a child still
    /// working or waiting for the user is left open.
    pub(super) fn close_threads(&mut self, lead: i64, ids: &[i64], cx: &mut Context<Self>) {
        let ids: Vec<i64> = ids
            .iter()
            .copied()
            .filter(|id| {
                self.thread_parents.get(id) == Some(&lead)
                    && self.session_summaries.contains_key(id)
                    && !self.thread_closed(*id, cx)
                    && self.thread_status(*id, cx)["status"] == "idle"
            })
            .collect();
        if ids.is_empty() {
            return;
        }
        for &id in &ids {
            self.set_thread_archived(id, true, cx);
        }
        self.thread_session_changed(lead, cx);
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            match locked(&store) {
                Ok(store) => ids
                    .into_iter()
                    .filter_map(|id| store.set_archived(id, true).err().map(|e| (id, e)))
                    .collect::<Vec<_>>(),
                Err(error) => ids.into_iter().map(|id| (id, error.clone())).collect(),
            }
        });
        cx.spawn(async move |this, cx| {
            let failed = work.await;
            let Some((_, error)) = failed.first().cloned() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                for (id, _) in &failed {
                    this.set_thread_archived(*id, false, cx);
                }
                this.thread_session_changed(lead, cx);
                this.error = Some(format!("Could not close agents: {error}"));
                cx.notify();
            });
        })
        .detach();
    }

    fn set_thread_archived(&mut self, id: i64, archived: bool, cx: &mut Context<Self>) {
        if let Some(summary) = self.session_summaries.get_mut(&id) {
            summary.archived = archived;
        }
        if let Some(view) = self.sessions.get(&id) {
            view.update(cx, |session, _| session.summary.archived = archived);
        }
        self.refresh_session_mark(id, cx);
    }

    fn release_thread_waiters(
        &mut self,
        id: i64,
        result: Result<Value, String>,
        cx: &mut Context<Self>,
    ) {
        for call in self.thread_waiters.remove(&id).unwrap_or_default() {
            self.finish_app_call(call, result.clone(), cx);
        }
    }

    pub(super) fn thread_session_changed(&mut self, id: i64, cx: &mut Context<Self>) {
        let lead = self.thread_parents.get(&id).copied().unwrap_or(id);
        let mut all: Vec<_> = self
            .thread_parents
            .iter()
            .filter(|(_, parent)| **parent == lead)
            .map(|(id, _)| *id)
            .filter(|id| self.session_summaries.contains_key(id))
            .collect();
        if all.is_empty() {
            return;
        }
        all.sort_unstable();
        // Closed children stay in the spawn tree's states, marked so it hides
        // them, but leave the rail.
        let children: Vec<i64> = all
            .iter()
            .copied()
            .filter(|&child| !self.thread_closed(child, cx))
            .collect();
        let stopped = self
            .sessions
            .get(&lead)
            .is_none_or(|v| v.read(cx).thread_stopping());
        if stopped {
            for &child in &all {
                if id == lead
                    && let Some(view) = self.sessions.get(&child)
                {
                    view.update(cx, |v, cx| v.stop_app_session(cx));
                }
                self.release_thread_waiters(child, Err("Lead thread was stopped".into()), cx);
            }
        } else {
            let status = self.thread_status(id, cx);
            if status["status"] != "working" {
                self.release_thread_waiters(id, Ok(status), cx);
            }
        }
        let states: HashMap<_, _> = all
            .iter()
            .map(|&child| {
                let mut status = self.thread_status(child, cx);
                if self.thread_closed(child, cx) {
                    status["closed"] = true.into();
                }
                (child, status)
            })
            .collect();
        if let Some(view) = self.sessions.get(&lead) {
            let transcript = view.read(cx).transcript.clone();
            transcript.update(cx, |t, cx| t.set_thread_states(states, cx));
        }
        for &child in &children {
            self.name_thread_senders(child, cx);
        }
        let mut tasks: Vec<_> = children
            .iter()
            .map(|&child| {
                let status = self.thread_status(child, cx);
                SummaryTask {
                    session: Some(child),
                    title: status["title"].as_str().unwrap_or("Child thread").into(),
                    provider: status["harness"].as_str().unwrap_or_default().into(),
                    model: status["model"].as_str().map(str::to_owned),
                    status: if status["status"] == "needsInput" {
                        "blocked"
                    } else if status["status"] == "working" {
                        "running"
                    } else if status["success"] == true {
                        "completed"
                    } else if status["success"] == false {
                        "failed"
                    } else {
                        "saved"
                    }
                    .into(),
                }
            })
            .collect();
        let live = self.sessions.get(&lead).is_some_and(|v| v.read(cx).busy)
            || tasks
                .iter()
                .any(|t| matches!(t.status.as_str(), "running" | "blocked"))
            || self.orchestrations.active(lead);
        if let Some(previous) = self.orchestration_summaries.get(&lead) {
            tasks.extend(
                previous
                    .tasks
                    .iter()
                    .filter(|t| {
                        t.session
                            .is_none_or(|child| !self.thread_parents.contains_key(&child))
                    })
                    .cloned(),
            );
        }
        let summary = Summary { live, tasks };
        self.set_agent_summary(lead, summary, cx);
        self.invalidate_chrome(cx);
        cx.notify();
    }
    /// The lead a subagent belongs to: the thread that started it with
    /// thread_start, or the Orchestrator whose run it works on. A thread
    /// opened with new_thread has none; it is the user's own.
    pub(crate) fn subagent_lead(&self, id: i64) -> Option<i64> {
        self.thread_parents
            .get(&id)
            .copied()
            .or_else(|| self.orchestration_lead(id))
    }
    /// A subagent opens in a pane beside its lead, never as a tab of its own,
    /// and one at a time: opening another takes the shown one's place.
    pub(super) fn open_subagent(
        &mut self,
        lead: i64,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigation_revision += 1;
        if self.workspace.tab_of(id).is_some()
            && self.workspace.tab_of(id) == self.workspace.tab_of(lead)
        {
            self.activate_session(id, window, cx);
            return;
        }
        if !self.session_summaries.contains_key(&lead) || self.deleting.contains(&lead) {
            // Its lead is gone; show it on its own rather than not at all.
            self.activate_session(id, window, cx);
            return;
        }
        if self.workspace.active != Some(lead) {
            self.replace_blank_pane(lead, cx);
            self.activate_session(lead, window, cx);
        }
        let Some(tab) = self.workspace.tab_of(lead) else {
            return;
        };
        let before = self.active_leaves();
        let shown = self
            .workspace
            .layout(tab)
            .leaf_ids()
            .into_iter()
            .find(|&pane| pane != id && self.subagent_lead(pane) == Some(lead));
        match shown {
            Some(shown) => {
                self.workspace.place(id, shown, Edge::Right);
                self.workspace.close_pane(shown);
            }
            None => self.workspace.place(id, lead, Edge::Right),
        }
        self.begin_pane_motion(before, cx);
        self.fit_sidebar(cx);
        self.activate_session(id, window, cx);
    }
    /// The lead's subagents for the strip above its composer.
    pub(super) fn subagent_strip(
        &self,
        lead: i64,
        cx: &App,
    ) -> Vec<crate::conversation::subagents::Subagent> {
        use crate::conversation::subagents::{Subagent, SubagentState};
        let Some(summary) = self.orchestration_summaries.get(&lead) else {
            return Vec::new();
        };
        summary
            .tasks
            .iter()
            .filter_map(|task| {
                let id = task.session?;
                let needs_input = self
                    .sessions
                    .get(&id)
                    .is_some_and(|view| view.read(cx).needs_approval());
                Some(Subagent {
                    id,
                    title: task.title.clone(),
                    provider: task.provider.clone(),
                    state: SubagentState::of(&task.status, summary.live, needs_input),
                })
            })
            .collect()
    }
    pub(super) fn push_subagent_strip(&self, lead: i64, cx: &mut Context<Self>) {
        let Some(view) = self.sessions.get(&lead).cloned() else {
            return;
        };
        let strip = self.subagent_strip(lead, cx);
        view.update(cx, |v, cx| v.set_subagents(strip, cx));
    }
    /// Every subagent of the lead, archived or closed ones included: its
    /// thread_start children, then its Orchestrator agents.
    pub(crate) fn subagents(&self, lead: i64) -> Vec<i64> {
        let mut ids: Vec<i64> = self
            .thread_parents
            .iter()
            .filter(|(_, parent)| **parent == lead)
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        if let Some(summary) = self.orchestration_summaries.get(&lead) {
            for id in summary.tasks.iter().filter_map(|t| t.session) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        ids
    }
    /// The thread that started `id`, as messages it sent there name it.
    pub(super) fn thread_senders(
        &self,
        id: i64,
    ) -> HashMap<i64, crate::transcript::threads::Sender> {
        self.started_by
            .get(&id)
            .and_then(|from| Some((*from, self.session_summaries.get(from)?)))
            .map(|(from, summary)| {
                let sender = crate::transcript::threads::Sender {
                    title: monocode_core::session_title::display_title(
                        &summary.title,
                        &summary.provider,
                    )
                    .replace('\n', " ")
                    .into(),
                    provider: summary.provider.clone(),
                };
                (from, sender)
            })
            .into_iter()
            .collect()
    }
    /// "from X" under a session another thread started.
    pub(super) fn started_from_line(&self, id: i64, cx: &App) -> Option<gpui::Div> {
        let from = *self.started_by.get(&id)?;
        if !self.session_summaries.contains_key(&from) && !self.sessions.contains_key(&from) {
            return None;
        }
        let title = self.session_metadata(from, cx).display_title();
        Some(
            div()
                .debug_selector(move || format!("session-origin-{id}"))
                .h(px(16.))
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(content(0.45))
                .child(icon("arrow-turn-down-right", 12.).text_color(content(0.4)))
                .child(div().min_w_0().truncate().child(format!("from {title}"))),
        )
    }
    pub(super) fn name_thread_senders(&self, id: i64, cx: &mut Context<Self>) {
        let senders = self.thread_senders(id);
        if let Some(view) = self.sessions.get(&id) {
            view.update(cx, |v, cx| {
                v.transcript.update(cx, |t, cx| {
                    if t.senders != senders {
                        t.senders = senders;
                        cx.notify();
                    }
                })
            });
        }
    }
    pub(super) fn set_agent_summary(
        &mut self,
        lead: i64,
        summary: Summary,
        cx: &mut Context<Self>,
    ) {
        if self.orchestration_summaries.get(&lead) == Some(&summary) {
            return;
        }
        let previous = self.agent_summary_writes.remove(&lead);
        let store = self.store.clone();
        let saved = summary.clone();
        let write = cx.spawn(async move |_, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            cx.background_executor()
                .spawn(async move {
                    let _ = locked(&store).and_then(|s| s.save_orchestration_summary(lead, &saved));
                })
                .await;
        });
        self.agent_summary_writes.insert(lead, write);
        self.orchestration_summaries.insert(lead, summary);
        self.push_subagent_strip(lead, cx);
        self.invalidate_chrome(cx);
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "macos"))]
mod shots;
