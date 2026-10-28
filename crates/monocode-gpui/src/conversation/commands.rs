//! Composer commands are handled before taking a draft or entering the turn queue.
use super::*;
use monocode_core::provider::{self, Event, Request};
use monocode_core::tool::{ToolCall, ToolState};
use monocode_core::transcript::Row;

impl Conversation {
    /// Returns true when this input belongs to a local command, including failures.
    /// A rejected command keeps its draft and attachments intact.
    pub(super) fn handle_local_command(&mut self, cx: &mut Context<Self>) -> bool {
        let text = self.input.read(cx).content.trim().to_owned();
        if text.eq_ignore_ascii_case("/mcp") {
            if self.input.read(cx).is_composing() {
                return true;
            }
            self.completion
                .update(cx, |completion, cx| completion.open_mcp(cx));
            return true;
        }
        if self.git_action_command(&text, cx) {
            return true;
        }
        if let Some(question) = monocode_core::composer::consume_command(&text, "btw") {
            let question = question.trim().to_owned();
            self.open_btw(question, false, cx);
            return true;
        }
        if !text.eq_ignore_ascii_case("/compact") {
            return false;
        }
        self.compact_context(cx);
        true
    }

    pub(super) fn compact_context(&mut self, cx: &mut Context<Self>) {
        if !self.history_ready() {
            self.defer_history_action(|this, cx| this.compact_context(cx), cx);
            return;
        }
        if self.editing_last_turn.pending()
            || self.stopping
            || self.attaching
            || self.input.read(cx).is_composing()
            || self.account_switching
            || self.controls.is_some()
            || self.needs_approval()
            || !self.questions.is_empty()
        {
            return;
        }
        if self.busy || !self.follow_ups.state.messages.is_empty() {
            self.status =
                "Wait for the current turn and queued messages before compacting context.".into();
            cx.notify();
            return;
        }
        let kind = Kind::from_id(&self.summary.provider);
        if !provider::supports_compact(kind) {
            self.status = format!(
                "Context compaction is not supported by the native {} transport.",
                kind.label()
            );
            cx.notify();
            return;
        }
        if self.summary.provider_id.is_none() {
            self.status = "Send a message before compacting this chat.".into();
            cx.notify();
            return;
        }
        if self.settings.worktree_removed || self.preparing_worktree || !self.loaded {
            return;
        }
        match locked(&self.store).and_then(|store| store.edit_recovery_reason(self.summary.id)) {
            Ok(Some(reason)) => {
                self.status = reason;
                cx.notify();
                return;
            }
            Err(error) => {
                self.status = error;
                cx.notify();
                return;
            }
            Ok(None) => {}
        }
        self.attach_account(cx);
        let mut settings = self.settings.clone();
        settings.environment.extend(self.app_environment.clone());
        settings.disable_claude_hooks = cx
            .try_global::<crate::settings::Preferences>()
            .is_some_and(|preferences| !preferences.claude_hooks);
        let cwd = self.execution_cwd().to_owned();
        let provider_id = self.summary.provider_id.clone();
        let model = self.model.clone();
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let mut context = self.context.unwrap_or_default();
        self.busy = true;
        self.submission_generation = self.submission_generation.wrapping_add(1);
        let generation = self.submission_generation;
        self.status = "Compacting context…".into();
        let compact_start = self.transcript.read(cx).source_rows().len();
        let mut compact_tool = ToolCall::new(
            &format!(
                "manual-compaction-{}-{generation}",
                monocode_core::time::now_ms()
            ),
            "contextCompaction",
        );
        let mut compact_row = Row {
            role: "tool".into(),
            text: serde_json::to_string(&compact_tool).unwrap(),
            first: true,
            sent_at: Some(monocode_core::time::now_ms()),
        };
        self.transcript.update(cx, |view, cx| {
            view.patch(compact_start, vec![compact_row.clone()], cx);
        });
        let compact_draft = self.input.read(cx).content.clone();
        let clear_command = compact_draft.trim().eq_ignore_ascii_case("/compact");
        let store = self.store.clone();
        let session = self.summary.id;
        let source_provider = self.summary.provider.clone();
        let source_account = self.settings.account.id.clone();
        let persist_provider = source_provider.clone();
        let persist_account = source_account.clone();
        self.completion
            .update(cx, |completion, cx| completion.dismiss(cx));
        let work = cx.background_executor().spawn(async move {
            let mut commands = None;
            let cancelled = cancel.clone();
            let result = (|| {
                locked(&store)?.write_rows(session, compact_start, &[compact_row.clone()])?;
                let binary = provider::resolve(kind).ok_or_else(|| kind.unavailable_hint())?;
                let request = Request {
                    kind,
                    binary,
                    cwd: cwd.into(),
                    prompt: String::new(),
                    provider_id,
                    model,
                    settings,
                };
                provider::compact(request, cancel, |events| {
                    for event in events {
                        match event {
                            Event::Context { used, window } => context.merge(used, window),
                            Event::Commands(discovered) => commands = Some(discovered),
                            _ => {}
                        }
                    }
                    Ok(())
                })?;
                let store = locked(&store)?;
                if store
                    .agent_settings(session)?
                    .is_some_and(|settings| settings.account.id == persist_account)
                    && store.sessions()?.iter().any(|current| {
                        current.id == session && current.provider == persist_provider
                    })
                {
                    store.save_context_usage(session, &context)?;
                }
                Ok(())
            })();
            compact_tool.state = if cancelled.load(Ordering::Relaxed) {
                ToolState::Cancelled
            } else if result.is_ok() {
                ToolState::Completed
            } else {
                ToolState::Failed
            };
            compact_tool.output = result.as_ref().err().cloned().unwrap_or_default();
            compact_row.text = serde_json::to_string(&compact_tool).unwrap();
            let saved = locked(&store).and_then(|mut store| {
                store.write_rows(session, compact_start, &[compact_row.clone()])
            });
            (result.and(saved), context, commands, compact_row)
        });
        cx.spawn(async move |this, cx| {
            let (result, context, commands, compact_row) = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.submission_generation != generation {
                    return;
                }
                this.busy = false;
                this.stopping = false;
                this.transcript.update(cx, |view, cx| {
                    view.patch(compact_start, vec![compact_row], cx);
                });
                let source_selected = this.summary.provider == source_provider
                    && this.settings.account.id == source_account;
                if let Some(commands) = commands.filter(|_| source_selected) {
                    this.completion.update(cx, |completion, cx| {
                        completion.set_native_commands(commands, cx)
                    });
                }
                if source_selected {
                    this.context = Some(context);
                }
                this.status = match result {
                    Ok(()) => {
                        if clear_command && this.input.read(cx).content == compact_draft {
                            this.input.update(cx, |input, cx| {
                                input.take(cx);
                            });
                        }
                        "Ready".into()
                    }
                    Err(error) if error == "Stopped" => "Ready".into(),
                    Err(error) => error,
                };
                if this.status == "Ready" {
                    this.run_history_actions(cx);
                    this.pump_follow_ups(cx);
                }
                cx.emit(SessionChanged);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
