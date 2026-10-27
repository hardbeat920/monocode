//! A paired phone's view of and commands for this chat, through the same paths
//! the composer and its pickers use.
use super::*;
use crate::notifications::approvals::InputKind;

/// What a chat waits on the user for, so the phone can say so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Waiting {
    Approval,
    Question,
}

impl Conversation {
    pub(super) fn sync_approvals(&mut self, approvals: Vec<Approval>, resolved: Vec<String>) {
        self.approvals.retain(|a| !resolved.contains(&a.id));
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        self.mobile_approval_tokens
            .retain(|id, _| self.approvals.iter().any(|a| &a.id == id));
        for approval in approvals {
            let token = format!(
                "{}-{}",
                monocode_core::time::now_ms(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            self.mobile_approval_tokens
                .insert(approval.id.clone(), token);
            if let Some(current) = self.approvals.iter_mut().find(|a| a.id == approval.id) {
                *current = approval;
            } else {
                self.approvals.push_back(approval);
            }
        }
    }

    pub(crate) fn mobile_approvals(&self) -> serde_json::Value {
        use serde_json::json;
        let mut remaining = 64_000;
        let mut approvals = self.approvals.iter().take(64).filter_map(|approval| {
            let token = self.mobile_approval_tokens.get(&approval.id)?;
            let mut decisions = vec![json!({"id":"deny","label":"Deny"})];
            if approval.can_remember {
                decisions.push(json!({"id":"session","label":"Allow for this session"}));
            }
            decisions.push(json!({"id":"allow","label":"Allow once"}));
            let id: String = approval.id.chars().take(512).collect();
            let title: String = approval.title.chars().take(512).collect();
            let limit = remaining.min(16_000);
            let detail: String = approval.detail.chars().take(limit).collect();
            remaining -= detail.chars().count();
            let truncated = id.len() < approval.id.len()
                || title.len() < approval.title.len() || detail.len() < approval.detail.len();
            let unsupported = if truncated {
                Some("Context is too large to show completely. Review and respond on the desktop.")
            } else if self.is_remote() {
                Some("Respond on the desktop: this remote connection cannot confirm mobile approval delivery.")
            } else {
                self.controls.is_none().then_some("The provider is no longer waiting. Resolve this on the desktop.")
            };
            Some(json!({"token":token,"requestId":id,"title":title,"detail":detail,
                "decisions":if unsupported.is_some() { vec![] } else { decisions }, "unsupported":unsupported}))
        }).collect::<Vec<_>>();
        if self.approvals.len() > 64 {
            approvals.push(json!({"token":"overflow","requestId":"", "title":"More approvals are waiting", "detail":"", "decisions":[], "unsupported":"Review the remaining approvals on the desktop."}));
        }
        json!(approvals)
    }

    pub(crate) fn mobile_respond_approval(
        &mut self,
        params: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.is_remote() {
            return Err("Respond on the desktop: this remote connection cannot confirm mobile approval delivery.".into());
        }
        let id = params["requestId"]
            .as_str()
            .ok_or("Invalid approval request ID")?;
        if self.mobile_approval_tokens.get(id).map(String::as_str) != params["token"].as_str()
            || !params["token"].is_string()
            || !self.approvals.iter().any(|a| a.id == id)
        {
            return Err("This approval was already resolved or replaced".into());
        }
        let advertised = self.mobile_approvals();
        let offered = advertised
            .as_array()
            .and_then(|items| items.iter().find(|a| a["token"] == params["token"]))
            .ok_or("Review this approval on the desktop")?;
        if let Some(reason) = offered["unsupported"].as_str() {
            return Err(reason.into());
        }
        let decision = match params["decision"].as_str() {
            Some("allow") => Decision::Allow,
            Some("deny") => Decision::Deny,
            Some("session") => Decision::Session,
            _ => return Err("This decision is not available for this approval".into()),
        };
        if !self.decide_notification_request(id, decision, cx) {
            return Err("The desktop could not deliver this decision. The approval may have changed; try again on the desktop.".into());
        }
        Ok(())
    }

    /// A phone's message, as if typed: it starts a turn, or while one runs it
    /// steers or queues. The desktop composer's own draft and files stay put.
    pub(crate) fn mobile_submit(
        &mut self,
        prompt: String,
        attachments: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if !self.history_ready() {
            self.defer_history_action(
                move |this, cx| this.mobile_submit(prompt, attachments, cx),
                cx,
            );
            return;
        }
        let composer_busy = self.attaching
            || !self.input.read(cx).content.is_empty()
            || !self.attachments.is_empty();
        if self.busy
            || self.stopping
            || self.preparing_worktree
            || composer_busy
            || self.editing_last_turn.pending()
            || self.settings.pending_provider_switch.is_some()
            || (!self.follow_ups.state.messages.is_empty() && !self.follow_ups.state.paused)
        {
            let composer = std::mem::replace(&mut self.attachments, attachments);
            self.enqueue_follow_up(prompt, None, cx);
            self.attachments = composer;
            if !self.busy {
                self.pump_follow_ups(cx);
            }
            cx.emit(SessionChanged);
            cx.notify();
            return;
        }
        self.attachments = attachments;
        self.input.update(cx, |i, cx| i.set_text(prompt, cx));
        self.send(cx);
        cx.emit(SessionChanged);
    }

    /// The phone's model and access sheets. A provider switch is armed as the
    /// desktop picker arms it, and hands off on the next send.
    pub(crate) fn mobile_configure(
        &mut self,
        model: Option<(Kind, Option<String>)>,
        model_settings: &std::collections::HashMap<String, String>,
        access: Option<AccessMode>,
        cx: &mut Context<Self>,
    ) {
        if let Some((kind, model)) = model
            && (self.summary.provider != kind.id() || self.model != model)
        {
            let picked = ModelPicked(kind, model);
            if self.apply_model_pick(&picked, cx) {
                remember_model(kind, picked.1, cx);
            }
        }
        for (key, value) in model_settings {
            match key.as_str() {
                "fast" => self.menu_choice(
                    if value == "true" {
                        "fast:on"
                    } else {
                        "fast:off"
                    },
                    cx,
                ),
                "thinking" => {
                    self.settings.thinking = Some(value == "true");
                    self.save_settings(cx);
                }
                key if is_effort_key(key) => self.menu_choice(&format!("effort:{value}"), cx),
                _ => {
                    if let Ok(choice) = serde_json::to_string(&(key, value)) {
                        self.menu_choice(&format!("model-setting:{choice}"), cx);
                    }
                }
            }
        }
        if let Some(access) = access {
            self.menu_choice(&format!("access:{}", access.id()), cx);
        }
        cx.emit(SessionChanged);
        cx.notify();
    }

    /// The current model id, as a picker would select it.
    pub(crate) fn mobile_model(&self) -> Option<&str> {
        self.model.as_deref().or(self.resolved_model.as_deref())
    }

    /// The model sheet's settings as the phone names them.
    pub(crate) fn mobile_model_settings(&self) -> std::collections::HashMap<String, String> {
        let mut values = self.settings.model_settings.clone();
        if let Some(effort) = &self.settings.effort {
            values.insert("effort".into(), effort.clone());
        }
        values.insert("fast".into(), self.settings.fast.to_string());
        if let Some(thinking) = self.settings.thinking {
            values.insert("thinking".into(), thinking.to_string());
        }
        values
    }

    /// The approvals and questions waiting on the desktop, for the phone's
    /// transcript; saved rows never record them.
    pub(crate) fn mobile_pending(&self, cx: &gpui::App) -> Vec<crate::mobile::transcript::Pending> {
        self.pending_notification_inputs(cx)
            .into_iter()
            .map(|input| crate::mobile::transcript::Pending {
                id: input.request_id,
                question: input.kind == InputKind::Question,
                label: input.label,
            })
            .collect()
    }

    pub(crate) fn mobile_waiting(&self, cx: &gpui::App) -> Option<Waiting> {
        let inputs = self.pending_notification_inputs(cx);
        if inputs.iter().any(|input| input.kind == InputKind::Approval) {
            Some(Waiting::Approval)
        } else if inputs.iter().any(|input| input.kind == InputKind::Question) {
            Some(Waiting::Question)
        } else {
            None
        }
    }
}

/// The permission modes a phone offers, with the desktop picker's hints.
pub(crate) fn mobile_access_modes()
-> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    ACCESS_MODES
        .iter()
        .map(|(id, label, hint, _)| (*id, *label, *hint))
}
