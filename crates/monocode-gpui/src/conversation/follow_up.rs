//! Composer.tsx / MessageQueue.tsx's durable follow-ups and saved user draft.
use super::*;
use monocode_core::follow_up::{Message, State};

#[derive(Default)]
pub(super) struct FollowUps {
    pub state: State,
    pub motion: super::composer_shelf::Motion,
    pub persisted: u64,
    pub sending: Option<String>,
    pub requested: Option<String>,
    pub continuations: std::collections::VecDeque<String>,
    pub resuming: bool,
    pub ready: bool,
    pub edit: Option<QueueEdit>,
}
/// The last composer draft written to the store, and the save waiting on a
/// pause in typing.
#[derive(Default)]
pub(super) struct ComposerDraft {
    pub saved: Option<Message>,
    pub pending: Option<gpui::Task<()>>,
}
pub(super) struct QueueEdit {
    id: String,
    input: Entity<input::Input>,
    _submit: Subscription,
}

impl Conversation {
    pub(super) fn message(
        &mut self,
        text: String,
        attachments: Vec<std::path::PathBuf>,
        mode: Option<String>,
        cx: &gpui::App,
    ) -> Message {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let prepared = self.completion.read(cx).prepare_prompt(&text);
        Message {
            prepared_prompt: (prepared != text).then_some(prepared),
            id: format!(
                "composer-{}-{}-{}",
                self.summary.id,
                monocode_core::time::now_ms(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            text,
            attachments,
            mode,
            ..Default::default()
        }
    }
    pub(super) fn save_follow_ups(&mut self, cx: &mut Context<Self>) {
        self.follow_ups.state.revision += 1;
        let mut snapshot = self.follow_ups.state.clone();
        let revision = snapshot.revision;
        let store = self.store.clone();
        let id = self.summary.id;
        let work = cx.background_executor().spawn(async move {
            let directory = locked(&store)?.data_dir().map(Path::to_path_buf);
            snapshot.retain_attachments(id, directory.as_deref())?;
            locked(&store)?.save_follow_ups(id, &snapshot)?;
            Ok::<_, String>(snapshot)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(snapshot) => {
                        if this.follow_ups.state.revision == revision {
                            this.follow_ups.state = snapshot;
                            let draft = this.follow_ups.state.draft.clone();
                            this.transcript
                                .update(cx, |view, cx| view.set_saved_draft(draft, cx));
                        }
                        this.follow_ups.persisted = this.follow_ups.persisted.max(revision);
                        this.pump_follow_ups(cx);
                    }
                    Err(error) if this.follow_ups.state.revision == revision => {
                        this.follow_ups.state.paused = true;
                        this.status = format!("Could not save messages: {error}");
                        if let Some(head) = this.follow_ups.state.messages.first_mut() {
                            head.error = Some(this.status.clone());
                        }
                    }
                    Err(_) => {}
                }
                cx.emit(SessionChanged);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn enqueue_follow_up(
        &mut self,
        prompt: String,
        mode: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.enqueue_follow_up_inverted(prompt, mode, false, cx);
    }
    /// Whether a plain follow-up sent now steers the turn rather than queuing;
    /// `invert` flips the preference, as Cmd+Enter does.
    pub(super) fn steers_follow_up(&self, invert: bool, cx: &gpui::App) -> bool {
        let prefers_steer = cx
            .try_global::<crate::settings::Preferences>()
            .is_none_or(|prefs| prefs.follow_up == "steer");
        // Once the agent has answered and only background work is left, which
        // may never end (a dev server), queuing would park the message behind
        // it, so hand it to the agent now.
        (prefers_steer != invert || !self.background.tasks.is_empty())
            && Kind::from_id(&self.summary.provider).can_steer()
    }
    /// `invert` flips the follow-up preference for this message (Cmd+Enter).
    pub(super) fn enqueue_follow_up_inverted(
        &mut self,
        prompt: String,
        mode: Option<String>,
        invert: bool,
        cx: &mut Context<Self>,
    ) {
        let attachments = std::mem::take(&mut self.attachments);
        let mut message = self.message(prompt, attachments, mode, cx);
        message.steer = message.can_steer() && self.steers_follow_up(invert, cx);
        if invert && self.busy && message.steer {
            self.follow_ups.requested = Some(message.id.clone());
        }
        self.follow_ups.motion.insert(
            &message.id,
            self.follow_ups.state.messages.is_empty(),
            !cx.reduce_motion(),
        );
        self.follow_ups.state.messages.push(message);
        self.save_follow_ups(cx);
    }
    /// The composer's unsent text and files: the thread's draft once it is left.
    fn composer_draft(&self, cx: &gpui::App) -> Option<Message> {
        let input = self.input.read(cx);
        (input.has_message_text() || !self.attachments.is_empty()).then(|| Message {
            text: input.content.to_string(),
            attachments: self.attachments.clone(),
            ..Default::default()
        })
    }
    pub(crate) fn composer_has_draft(&self) -> bool {
        self.draft_has_value || !self.attachments.is_empty()
    }
    /// A launch's saved prompt, as an Operator launch that sends right away
    /// leaves it until the turn is accepted.
    #[cfg(test)]
    pub(crate) fn hold_saved_draft(&mut self, draft: Message) {
        self.follow_ups.state.draft = Some(draft);
    }
    /// A saved draft or unsent composer text that an Operator must not overwrite.
    pub(crate) fn has_unsent_draft(&self) -> bool {
        self.has_saved_draft() || self.composer_has_draft()
    }
    /// Saved drafts, from an Operator draft launch, a remote host or from
    /// before drafts lived in the composer, open in the composer so a thread
    /// has one kind of draft. A launch waiting to send its draft keeps it.
    pub(super) fn adopt_saved_draft(&mut self, cx: &mut Context<Self>) {
        let remote = self.is_remote();
        if !self.history_actions.is_empty()
            || !self.input.read(cx).content.is_empty()
            || !self.attachments.is_empty()
            || (remote
                && (self.busy
                    || self.stopping
                    || self.remote_state.pending
                    || !self.remote_state.online))
        {
            return;
        }
        let Some(draft) = self.follow_ups.state.draft.clone() else {
            return;
        };
        self.attachments = draft.attachments;
        self.input
            .update(cx, |input, cx| input.set_text(draft.text, cx));
        // The composer's copy is written first, so a crash between the two
        // writes cannot lose the draft.
        self.save_composer_draft(cx);
        if remote {
            // Removing the host's copy leaves the composer's as the only one.
            self.remote_draft_action(false, cx);
            self.follow_ups.state.draft = None;
        } else {
            self.follow_ups.state.draft = None;
            self.save_follow_ups(cx);
        }
        self.transcript
            .update(cx, |view, cx| view.set_saved_draft(None, cx));
    }
    /// Reopens the thread with what was left in its composer. Files that are
    /// gone, such as pasted images cleared from the temp folder, are dropped.
    pub(crate) fn restore_composer_draft(&mut self, mut draft: Message, cx: &mut Context<Self>) {
        self.composer_draft.saved = Some(draft.clone());
        if !self.input.read(cx).content.is_empty() || !self.attachments.is_empty() {
            return;
        }
        draft.attachments.retain(|path| path.exists());
        self.attachments = draft.attachments;
        self.input
            .update(cx, |input, cx| input.set_text(draft.text, cx));
    }
    /// Typing saves the draft once it pauses. Leaving the composer and quitting
    /// save it at once, so only a crash mid-word can lose the last keystrokes.
    pub(super) fn queue_composer_draft(&mut self, cx: &mut Context<Self>) {
        // The input also notifies for caret and focus changes.
        if self.composer_draft.pending.is_none()
            && self.composer_draft(cx) == self.composer_draft.saved
        {
            return;
        }
        self.composer_draft.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            let _ = this.update(cx, |this, cx| this.save_composer_draft(cx));
        }));
    }
    /// Writes a save still waiting on the typing pause.
    pub(crate) fn flush_composer_draft(&mut self, cx: &mut Context<Self>) {
        if self.composer_draft.pending.is_some() {
            self.save_composer_draft(cx);
        }
    }
    /// One small row, written in place so saves can never land out of order.
    pub(super) fn save_composer_draft(&mut self, cx: &mut Context<Self>) {
        self.composer_draft.pending = None;
        let draft = self.composer_draft(cx);
        if draft != self.composer_draft.saved
            && locked(&self.store)
                .and_then(|s| s.save_composer_draft(self.summary.id, draft.as_ref()))
                .is_ok()
        {
            self.composer_draft.saved = draft;
        }
    }
    pub(super) fn draft_action(&mut self, send: bool, cx: &mut Context<Self>) {
        if self.is_remote() {
            self.remote_draft_action(send, cx);
            return;
        }
        if self.needs_history_restore() {
            self.defer_history_action(move |this, cx| this.draft_action(send, cx), cx);
            return;
        }
        if send && (!self.loaded || self.preparing_worktree) {
            return;
        }
        let Some(mut message) = self.follow_ups.state.draft.take() else {
            return;
        };
        if send {
            message.steer = cx
                .try_global::<crate::settings::Preferences>()
                .is_none_or(|prefs| prefs.follow_up == "steer")
                && Kind::from_id(&self.summary.provider).can_steer();
            if !self.busy {
                self.follow_ups.requested = Some(message.id.clone());
            }
            self.follow_ups.motion.insert(
                &message.id,
                self.follow_ups.state.messages.is_empty(),
                !cx.reduce_motion(),
            );
            self.follow_ups.state.messages.push(message);
        }
        self.summary.has_messages = !self.transcript.read(cx).source_rows().is_empty() || send;
        self.transcript
            .update(cx, |view, cx| view.set_saved_draft(None, cx));
        self.save_follow_ups(cx);
        cx.emit(SessionChanged);
    }
    pub(super) fn follow_up_delivered(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.follow_ups.sending.as_deref() == Some(id) {
            self.follow_ups.sending = None;
        }
        if self.follow_ups.requested.as_deref() == Some(id) {
            self.follow_ups.requested = None;
        }
        self.follow_ups
            .continuations
            .retain(|message| message != id);
        if self.follow_ups.requested.is_none() {
            self.follow_ups.requested = self.follow_ups.continuations.front().cloned();
        }
        let before = self.follow_ups.state.messages.len();
        self.follow_ups
            .state
            .messages
            .retain(|message| message.id != id);
        if before != self.follow_ups.state.messages.len() {
            self.save_follow_ups(cx);
        }
    }
    pub(super) fn fail_follow_up(&mut self, id: &str, error: String, cx: &mut Context<Self>) {
        if let Some(message) = self
            .follow_ups
            .state
            .messages
            .iter_mut()
            .find(|m| m.id == id)
        {
            message.error = Some(error);
            self.follow_ups.state.paused = true;
            if self
                .follow_ups
                .continuations
                .iter()
                .any(|message| message == id)
            {
                self.follow_ups.continuations.clear();
            }
            if self.follow_ups.sending.as_deref() == Some(id) {
                self.follow_ups.sending = None;
            }
            if self.follow_ups.requested.as_deref() == Some(id) {
                self.follow_ups.requested = None;
            }
            self.save_follow_ups(cx);
        }
    }
    pub(super) fn pump_follow_ups(&mut self, cx: &mut Context<Self>) {
        if self.editing_last_turn.pending() || self.stopping {
            return;
        }
        if !self.history_ready() {
            // Persistence callbacks can arrive while a hidden history reloads.
            // The load completion pumps the durable queue after restoring rows.
            return;
        }
        if self.settings.usage_limit.is_some()
            || self.settings.worktree_removed
            || self.account_switching
            || self.settings.mono.is_some()
            || !self.loaded
            || self.preparing_worktree
            || (self.follow_ups.state.paused && self.follow_ups.requested.is_none())
            || self.follow_ups.sending.is_some()
            || self.follow_ups.resuming
            || self.follow_ups.persisted < self.follow_ups.state.revision
        {
            return;
        }
        let message = self
            .follow_ups
            .requested
            .as_ref()
            .and_then(|id| self.follow_ups.state.messages.iter().find(|m| &m.id == id))
            .or_else(|| {
                self.busy
                    .then(|| self.follow_ups.state.messages.iter().find(|m| m.steer))
                    .flatten()
            })
            .or_else(|| self.follow_ups.state.messages.first());
        let Some(message) = message.cloned() else {
            return;
        };
        if self
            .follow_ups
            .edit
            .as_ref()
            .is_some_and(|edit| edit.id == message.id)
        {
            return;
        }
        if self.busy {
            let requested = self.follow_ups.requested.as_deref() == Some(message.id.as_str());
            if self.settings.pending_provider_switch.is_some() {
                if requested {
                    self.stop(cx);
                    self.follow_ups.requested = Some(message.id.clone());
                    self.follow_ups.continuations.push_back(message.id);
                }
                return;
            }
            if !message.steer
                || !Kind::from_id(&self.summary.provider).can_steer()
                || !message.can_steer()
                || !self.follow_ups.ready
                || (!requested
                    && self
                        .questions
                        .iter()
                        .any(|(card, _)| card.read(cx).blocks_steering()))
            {
                return;
            }
            if let Some(controls) = &self.controls {
                if controls
                    .send(Control {
                        id: message.id.clone(),
                        decision: Decision::Steer(message.clone()),
                    })
                    .is_ok()
                {
                    self.follow_ups.sending = Some(message.id);
                } else {
                    self.fail_follow_up(
                        &message.id,
                        "The provider disconnected before accepting this message".into(),
                        cx,
                    );
                }
            }
        } else {
            if self.settings.new_worktree && self.settings.worktree_cwd.is_none() {
                self.prepare_worktree(cx);
                return;
            }
            self.follow_ups.sending = Some(message.id.clone());
            let message_id = message.id.clone();
            self.settings.mono_message_id = Some(message.id.clone());
            self.launch_message(message, cx);
            self.settings.mono_message_id = None;
            if !self.busy {
                self.fail_follow_up(&message_id, self.status.clone(), cx);
            }
        }
        cx.notify();
    }
    /// Starts `message` with its own one-shot mode, attachments and prompt.
    /// The user's next composer selection must not change a message already
    /// queued, and the message must not take the composer's.
    pub(super) fn launch_message(&mut self, message: Message, cx: &mut Context<Self>) {
        let previous = (
            self.settings.plan,
            self.settings.operator,
            self.settings.orchestrator,
        );
        self.settings.plan = message.mode.as_deref() == Some("plan");
        self.settings.operator = message.mode.as_deref() == Some("operator");
        self.settings.orchestrator = message.mode.as_deref() == Some("orchestrator");
        self.planning_orchestration = self.settings.orchestrator;
        self.orchestration_plan = None;
        self.orchestration_error = None;
        let composer_attachments = std::mem::take(&mut self.attachments);
        self.settings.attachments = message.attachments;
        let previous_prepared = self.settings.prepared_prompt.take();
        self.settings.prepared_prompt = message.prepared_prompt;
        self.launch_prompt(message.text, cx);
        self.settings.prepared_prompt = previous_prepared;
        self.attachments = composer_attachments;
        self.settings.attachments.clear();
        (
            self.settings.plan,
            self.settings.operator,
            self.settings.orchestrator,
        ) = previous;
    }
    fn resume_queue(&mut self, cx: &mut Context<Self>) {
        if self.stopping {
            self.defer_history_action(|this, cx| this.resume_queue(cx), cx);
            return;
        }
        if self.busy || !self.follow_ups.state.paused || self.follow_ups.state.messages.is_empty() {
            return;
        }
        // Resume the interrupted provider turn before releasing its outbox,
        // matching App.tsx's queueStatus = "resuming" continuation.
        self.follow_ups.resuming = true;
        self.launch_prompt("Continue from where you left off.".into(), cx);
        if !self.busy {
            self.follow_ups.resuming = false;
        }
        cx.notify();
    }
    fn edit_queued(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.follow_ups.sending.as_ref() == Some(&id) {
            return;
        }
        let Some(message) = self
            .follow_ups
            .state
            .messages
            .iter()
            .find(|message| message.id == id)
        else {
            return;
        };
        let text = message.text.clone();
        let input = cx.new(|cx| {
            let mut input = input::Input::queued_message(cx);
            input.set_text(text, cx);
            input
        });
        let submit = cx.subscribe_in(
            &input,
            window,
            |this, _, _: &input::Submitted, window, cx| this.finish_queue_edit(true, window, cx),
        );
        window.focus(&input.focus_handle(cx), cx);
        self.follow_ups.edit = Some(QueueEdit {
            id,
            input,
            _submit: submit,
        });
        cx.notify();
    }
    fn finish_queue_edit(&mut self, save: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.follow_ups.edit else {
            return;
        };
        if edit.input.read(cx).is_composing() {
            return;
        }
        if save {
            let text = edit.input.read(cx).content.to_string();
            let prepared = self.completion.read(cx).prepare_prompt(&text);
            if let Some(message) = self
                .follow_ups
                .state
                .messages
                .iter_mut()
                .find(|m| m.id == edit.id)
            {
                if text.trim().is_empty() && message.attachments.is_empty() {
                    return;
                }
                message.replace_text(text, prepared);
                message.error = None;
            }
        }
        self.follow_ups.edit = None;
        window.focus(&self.input.focus_handle(cx), cx);
        if save {
            self.save_follow_ups(cx);
        } else {
            self.pump_follow_ups(cx);
        }
        cx.notify();
    }
    pub(super) fn message_queue(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let width = self.composer_anchor.get().map(|bounds| bounds.size.width);
        self.follow_ups.motion.retain(
            self.follow_ups
                .state
                .messages
                .iter()
                .map(|message| message.id.as_str()),
            !cx.reduce_motion() && width.is_some(),
        );
        if self.follow_ups.state.messages.is_empty() {
            return div().into_any_element();
        }
        let card_width = (width.unwrap_or_default() - px(16.)).max(px(0.));
        let row_width = (card_width - px(22.)).max(px(0.));
        let failed = self
            .follow_ups
            .state
            .messages
            .iter()
            .any(|m| m.error.is_some());
        let card = composer_shelf::card("message-queue-card", self.classic_layout)
            .when(self.follow_ups.state.paused && !self.follow_ups.resuming, |d| d.child(
                div().h(px(28.)).flex().items_center().gap_2().border_b_1().border_color(content(0.1))
                    .child(icon("queue-pause", 14.))
                    .child(div().flex_1().min_w_0().truncate().child(if failed { "A message couldn't be sent" } else { "Queue paused because you interrupted" }))
                    .child(queue_button("queue-resume", if failed { "Retry queue" } else { "Resume queue" }, "queue-play").w_auto().px(px(6.)).gap(px(6.)).child(if failed { "Retry" } else { "Resume" }).on_click(cx.listener(|this, _, _, cx| this.resume_queue(cx))))
            ))
            .children(self.follow_ups.state.messages.iter().enumerate().map(|(index, message)| {
                let id = message.id.clone();
                let editing = self.follow_ups.edit.as_ref().filter(|edit| edit.id == id);
                let sending = self.follow_ups.sending.as_ref() == Some(&id);
                div().id(SharedString::from(format!("queued-{id}"))).role(Role::Group).aria_label(format!("Queued message: {}", message.label())).min_h(px(32.)).flex().items_center().gap(px(10.)).when(index > 0, |d| d.border_t_1().border_color(content(0.05)))
                    .child(icon("queue-list", 14.).text_color(content(0.35)))
                    .map(|row| if let Some(edit) = editing {
                        row.child(div().flex_1().min_w_0().px(px(6.)).py(px(2.)).rounded_md().border_1().border_color(content(0.3)).bg(content(0.05)).on_action(cx.listener(|this, _: &crate::shell::DismissMenu, window, cx| { this.finish_queue_edit(false, window, cx); cx.stop_propagation(); })).child(edit.input.clone()))
                            .child(queue_button("queue-save", "Save queued message", "check").on_click(cx.listener(|this, _, window, cx| this.finish_queue_edit(true, window, cx))))
                            .child(queue_button("queue-cancel", "Cancel queued message edit", "close").on_click(cx.listener(|this, _, window, cx| this.finish_queue_edit(false, window, cx))))
                    } else {
                        let (steer, edit, remove) = (id.clone(), id.clone(), id);
                        row.child(div().flex_1().min_w_0().py_1().child(div().id("queued-message-text").role(Role::Label).aria_label(message.label()).truncate().text_color(content(0.75)).child(message.label())).when_some(message.error.clone(), |d, error| d.child(div().id("queue-error").role(Role::Alert).text_color(gpui::rgb(0xf87171)).child(error))))
                            .when(Kind::from_id(&self.summary.provider).can_steer(), |d| d.child(queue_button("queue-steer", "Steer queued message", "queue-steer").w_auto().px_2().gap(px(6.)).opacity(if sending { 0.4 } else { 1. }).child("Steer").on_click(cx.listener(move |this, _, _, cx| {
                                if !sending && let Some(message) = this.follow_ups.state.messages.iter_mut().find(|m| m.id == steer) {
                                    if !message.can_steer() && this.busy { return; }
                                    message.steer = true;
                                    message.error = None;
                                    this.follow_ups.requested = Some(steer.clone());
                                    this.save_follow_ups(cx);
                                }
                            }))))
                            .child(queue_button("queue-edit", "Edit queued message", "pencil-edit").opacity(if sending { 0.4 } else { 1. }).on_click(cx.listener(move |this, _, window, cx| { if !sending { this.edit_queued(edit.clone(), window, cx); } })))
                            .child(queue_button("queue-remove", "Remove queued message", "trash").opacity(if sending { 0.4 } else { 1. }).on_click(cx.listener(move |this, _, _, cx| {
                                if !sending {
                                    this.follow_ups.state.messages.retain(|m| m.id != remove);
                                    this.follow_ups.continuations.retain(|id| id != &remove);
                                    if this.follow_ups.requested.as_ref() == Some(&remove) { this.follow_ups.requested = this.follow_ups.continuations.front().cloned(); }
                                    this.save_follow_ups(cx);
                                }
                            })))
                    })
                    .map(|row| self.follow_ups.motion.row(&message.id, row, row_width, window))
            }));
        composer_shelf::shelf("message-queue")
            .child(self.follow_ups.motion.card(card, card_width, window))
            .into_any_element()
    }
}

fn queue_button(
    id: &'static str,
    label: &'static str,
    glyph: &'static str,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .size(px(24.))
        .flex_shrink_0()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .text_color(content(0.5))
        .hover(|s| s.bg(content(0.08)).text_color(content(0.9)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(icon(glyph, 14.))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowHandle};
    use monocode_core::store::Store;
    use std::sync::Mutex;

    fn conversation(cx: &mut TestAppContext) -> WindowHandle<Conversation> {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("codex", "/tmp").unwrap();
        cx.update(|cx| {
            input::bind(cx);
            cx.set_global(crate::settings::Preferences {
                follow_up: "queue".into(),
                ..Default::default()
            });
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view
                })
            })
            .unwrap()
        })
    }

    #[gpui::test]
    fn queue_entrances_slide_above_a_stationary_composer_for_the_first_and_later_messages(
        cx: &mut TestAppContext,
    ) {
        use std::time::Duration;
        let window = conversation(cx);
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                view.summary.has_messages = true;
                view.busy = true;
                view.follow_ups.state.paused = true;
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        let bounds = |window: WindowHandle<Conversation>, cx: &mut TestAppContext| {
            window
                .update(cx, |view, window, _| {
                    let composer = window
                        .painted_quads()
                        .into_iter()
                        .find(|quad| {
                            (quad.corner_radii.top_left.0 / window.scale_factor() - 12.).abs() < 0.1
                                && quad.bounds.size.width.0 / window.scale_factor() > 300.
                        })
                        .expect("the composer surface must be painted");
                    (
                        f32::from(view.composer_anchor.get().unwrap().top()),
                        composer.bounds.top().0 / window.scale_factor(),
                    )
                })
                .unwrap()
        };
        let (mut queue_top, input_top) = bounds(window, cx);
        for (index, text) in ["First queued message", "A second queued message"]
            .into_iter()
            .enumerate()
        {
            window
                .update(cx, |view, _, cx| {
                    view.enqueue_follow_up(text.into(), None, cx);
                    view.follow_ups.motion.elapsed(Duration::ZERO);
                    cx.notify();
                })
                .unwrap();
            cx.refresh().unwrap();
            let (starting, composer) = bounds(window, cx);
            assert!(
                (composer - input_top).abs() < 1.,
                "the input must stay still"
            );
            assert!(
                (starting - queue_top).abs() < 8.,
                "the entrance starts behind the input"
            );

            window
                .update(cx, |view, _, cx| {
                    view.follow_ups.motion.elapsed(Duration::from_millis(40));
                    cx.notify();
                })
                .unwrap();
            cx.refresh().unwrap();
            let (middle, composer) = bounds(window, cx);
            assert!((composer - input_top).abs() < 1.);
            assert!(middle < starting - 5., "the queue must slide upward");

            window
                .update(cx, |view, _, cx| {
                    view.follow_ups.motion.elapsed(Duration::from_secs(1));
                    cx.notify();
                })
                .unwrap();
            cx.refresh().unwrap();
            let (settled, composer) = bounds(window, cx);
            assert!((composer - input_top).abs() < 1.);
            assert!(
                settled < middle - 5.,
                "the spring must settle into the full row height"
            );
            assert!(queue_top - settled >= if index == 0 { 60. } else { 30. });
            queue_top = settled;
        }
        cx.update(|cx| cx.set_reduce_motion(true));
        window
            .update(cx, |view, _, cx| {
                view.enqueue_follow_up("Reduced motion".into(), None, cx);
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        let (settled, composer) = bounds(window, cx);
        assert!((composer - input_top).abs() < 1.);
        assert!(
            queue_top - settled >= 30.,
            "Reduce Motion must reveal the full row immediately"
        );
    }

    #[gpui::test]
    fn submitted_modes_clear_after_queueing_and_do_not_leak_into_the_next_message(
        cx: &mut TestAppContext,
    ) {
        for mode in ["operator", "orchestrator", "plan"] {
            let window = conversation(cx);
            cx.run_until_parked();
            window
                .update(cx, |view, _, cx| {
                    view.busy = true;
                    view.follow_ups.state.paused = true;
                    view.toggle_mode(mode, cx);
                    let selected = view.input.read(cx).content.clone();
                    view.send(cx);
                    assert_eq!(
                        view.input.read(cx).content,
                        selected,
                        "an empty submission keeps its mode"
                    );
                    view.input
                        .update(cx, |i, cx| i.set_text(format!("/{mode} first message"), cx));
                    view.send(cx);
                    assert!(view.input.read(cx).content.is_empty());
                    assert!(
                        !view.settings.plan
                            && !view.settings.operator
                            && !view.settings.orchestrator
                    );
                    view.input
                        .update(cx, |i, cx| i.set_text("ordinary follow-up".into(), cx));
                    view.send(cx);
                    let messages = &view.follow_ups.state.messages;
                    assert_eq!(messages[0].mode.as_deref(), Some(mode));
                    assert_eq!(messages[0].text, "first message");
                    assert_eq!(messages[1].mode, None);
                    assert_eq!(messages[1].text, "ordinary follow-up");
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |view, _, _| {
                    let store = locked(&view.store).unwrap();
                    let saved = store.agent_settings(view.summary.id).unwrap().unwrap();
                    assert!(!saved.plan && !saved.operator && !saved.orchestrator);
                    let queue = store.follow_ups(view.summary.id).unwrap();
                    assert_eq!(queue.messages[0].mode.as_deref(), Some(mode));
                    assert_eq!(queue.messages[1].mode, None);
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn busy_sends_keep_their_order_attachments_and_edits_until_accepted(cx: &mut TestAppContext) {
        let window = conversation(cx);
        let (controls, received) = std::sync::mpsc::channel();
        window
            .update(cx, |view, _, cx| {
                view.busy = true;
                view.controls = Some(controls);
                view.input
                    .update(cx, |i, cx| i.set_text("first".into(), cx));
                view.attachments.push("/tmp/image.png".into());
                view.send(cx);
                view.input
                    .update(cx, |i, cx| i.set_text("second".into(), cx));
                view.send(cx);
                assert!(view.input.read(cx).content.is_empty());
                assert_eq!(
                    view.follow_ups
                        .state
                        .messages
                        .iter()
                        .map(|m| m.text.as_str())
                        .collect::<Vec<_>>(),
                    ["first", "second"]
                );
            })
            .unwrap();
        cx.run_until_parked();
        assert!(received.try_recv().is_err());
        window
            .update(cx, |view, w, cx| {
                let id = view.follow_ups.state.messages[0].id.clone();
                view.edit_queued(id, w, cx);
                view.follow_ups
                    .edit
                    .as_ref()
                    .unwrap()
                    .input
                    .update(cx, |i, cx| i.set_text("edited 🙂\nwith detail".into(), cx));
                view.follow_ups.ready = true;
                view.follow_ups.state.messages[0].steer = true;
                view.pump_follow_ups(cx);
                assert!(
                    received.try_recv().is_err(),
                    "editing the head must block delivery"
                );
                view.finish_queue_edit(true, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        let control = received.try_recv().unwrap();
        let Decision::Steer(message) = control.decision else {
            panic!("expected steering");
        };
        assert_eq!(message.text, "edited 🙂\nwith detail");
        assert_eq!(
            message.attachments,
            [std::path::PathBuf::from("/tmp/image.png")]
        );
        window
            .update(cx, |view, _, cx| {
                assert_eq!(
                    view.follow_ups.state.messages.len(),
                    2,
                    "a write to the control channel is not an acceptance"
                );
                view.follow_up_delivered(&message.id, cx);
                assert_eq!(view.follow_ups.state.messages[0].text, "second");
                assert!(view.follow_ups.sending.is_none());
            })
            .unwrap();
    }

    #[gpui::test]
    fn stop_pauses_the_queue_and_retires_its_controls(cx: &mut TestAppContext) {
        let window = conversation(cx);
        let (controls, received) = std::sync::mpsc::channel();
        window
            .update(cx, |view, _, cx| {
                view.busy = true;
                view.controls = Some(controls);
                view.input
                    .update(cx, |input, cx| input.set_text("keep this".into(), cx));
                view.send(cx);
                view.stop(cx);
                assert!(view.stopping);
                assert!(view.follow_ups.state.paused);
                assert!(!view.follow_ups.ready);
                assert!(view.controls.is_none());
                assert_eq!(view.follow_ups.state.messages[0].text, "keep this");
            })
            .unwrap();
        cx.run_until_parked();
        assert!(matches!(
            received.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
    }

    #[gpui::test]
    fn a_rejected_steer_keeps_its_text_for_retry(cx: &mut TestAppContext) {
        let window = conversation(cx);
        cx.update(|cx| {
            cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                prefs.follow_up = "steer".into()
            });
        });
        let (controls, received) = std::sync::mpsc::channel();
        window
            .update(cx, |view, _, cx| {
                view.busy = true;
                view.controls = Some(controls);
                view.follow_ups.ready = true;
                view.input
                    .update(cx, |input, cx| input.set_text("keep this".into(), cx));
                view.send(cx);
            })
            .unwrap();
        cx.run_until_parked();
        let id = received.try_recv().unwrap().id;
        window
            .update(cx, |view, _, cx| {
                view.fail_follow_up(&id, "Rejected by provider".into(), cx);
                assert!(view.follow_ups.state.paused);
                assert_eq!(view.follow_ups.state.messages[0].text, "keep this");
                assert_eq!(
                    view.follow_ups.state.messages[0].error.as_deref(),
                    Some("Rejected by provider")
                );
            })
            .unwrap();
    }

    fn alternate_send(cx: &mut TestAppContext, follow_up: &str, text: &str) -> bool {
        let window = conversation(cx);
        cx.update(|cx| {
            cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                prefs.follow_up = follow_up.into()
            })
        });
        window
            .update(cx, |view, window, cx| {
                view.busy = true;
                view.input
                    .update(cx, |input, cx| input.set_text(text.into(), cx));
                window.focus(&view.input.focus_handle(cx), cx);
            })
            .unwrap();
        cx.run_until_parked();
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.simulate_keystrokes(window.into(), &format!("{modifier}-enter"));
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert!(view.input.read(cx).content.is_empty());
                let messages = &view.follow_ups.state.messages;
                assert_eq!(messages.len(), 1);
                assert!(text.ends_with(messages[0].text.as_str()));
                messages[0].steer
            })
            .unwrap()
    }

    #[gpui::test]
    fn the_submit_icon_shows_whether_enter_steers_or_queues(cx: &mut TestAppContext) {
        let window = conversation(cx);
        let action = |cx: &mut TestAppContext, follow_up: &str, alternate: bool| {
            cx.update(|cx| {
                cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                    prefs.follow_up = follow_up.into()
                })
            });
            window
                .update(cx, |view, _, cx| view.follow_up_action(alternate, cx))
                .unwrap()
                .map(|(label, _)| label)
        };
        assert_eq!(action(cx, "steer", false), None, "idle sends start a turn");
        window.update(cx, |view, _, _| view.busy = true).unwrap();
        assert_eq!(action(cx, "steer", false), Some("Steer"));
        assert_eq!(action(cx, "steer", true), Some("Queue"));
        assert_eq!(action(cx, "queue", false), Some("Queue"));
        assert_eq!(action(cx, "queue", true), Some("Steer"));
        window
            .update(cx, |view, _, _| view.settings.plan = true)
            .unwrap();
        assert_eq!(
            action(cx, "queue", true),
            Some("Queue"),
            "modes always queue"
        );
    }

    #[gpui::test]
    fn alternate_steer_delivers_past_queued_message(cx: &mut TestAppContext) {
        let window = conversation(cx);
        let (controls, received) = std::sync::mpsc::channel();
        window
            .update(cx, |view, _, cx| {
                view.busy = true;
                view.controls = Some(controls);
                view.follow_ups.ready = true;
                view.input
                    .update(cx, |input, cx| input.set_text("queued".into(), cx));
                view.send(cx);
                view.input
                    .update(cx, |input, cx| input.set_text("steering".into(), cx));
                view.send_follow_up(true, cx);
            })
            .unwrap();
        cx.run_until_parked();
        let control = received
            .try_recv()
            .expect("steering message must be delivered");
        let Decision::Steer(message) = control.decision else {
            panic!("expected steering")
        };
        assert_eq!(message.text, "steering");
        window
            .update(cx, |view, _, cx| {
                view.follow_up_delivered(&message.id, cx);
                assert_eq!(view.follow_ups.state.messages[0].text, "queued");
            })
            .unwrap();
    }

    #[gpui::test]
    fn plain_steer_delivers_past_queued_message(cx: &mut TestAppContext) {
        let window = conversation(cx);
        cx.update(|cx| {
            cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                prefs.follow_up = "steer".into()
            })
        });
        let (controls, received) = std::sync::mpsc::channel();
        window
            .update(cx, |view, _, cx| {
                view.busy = true;
                view.controls = Some(controls);
                view.follow_ups.ready = true;
                view.input
                    .update(cx, |input, cx| input.set_text("queued".into(), cx));
                view.send_follow_up(true, cx);
                view.input
                    .update(cx, |input, cx| input.set_text("steering".into(), cx));
                view.send(cx);
            })
            .unwrap();
        cx.run_until_parked();
        let control = received
            .try_recv()
            .expect("steering message must be delivered");
        let Decision::Steer(message) = control.decision else {
            panic!("expected steering")
        };
        assert_eq!(message.text, "steering");
        window
            .update(cx, |view, _, cx| {
                view.follow_up_delivered(&message.id, cx);
                assert_eq!(view.follow_ups.state.messages[0].text, "queued");
            })
            .unwrap();
    }

    #[gpui::test]
    fn modifier_click_inverts_both_follow_up_preferences(cx: &mut TestAppContext) {
        for (preference, expected) in [("steer", false), ("queue", true)] {
            let window = conversation(cx);
            cx.update(|cx| {
                cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                    prefs.follow_up = preference.into()
                })
            });
            window
                .update(cx, |view, _, cx| {
                    view.summary.has_messages = true;
                    view.busy = true;
                    view.input
                        .update(cx, |input, cx| input.set_text("message".into(), cx));
                    cx.notify();
                })
                .unwrap();
            let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
            visual.refresh().unwrap();
            let bounds = visual.debug_bounds("send-or-stop").unwrap();
            let modifiers = gpui::Modifiers {
                platform: cfg!(target_os = "macos"),
                control: !cfg!(target_os = "macos"),
                ..Default::default()
            };
            visual.simulate_click(bounds.center(), modifiers);
            visual.run_until_parked();
            window
                .update(visual, |view, _, _| {
                    assert_eq!(view.follow_ups.state.messages[0].steer, expected)
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn cmd_enter_queues_when_follow_ups_steer(cx: &mut TestAppContext) {
        assert!(!alternate_send(cx, "steer", "after this turn"));
    }

    #[gpui::test]
    fn cmd_enter_steers_when_follow_ups_queue(cx: &mut TestAppContext) {
        assert!(alternate_send(cx, "queue", "change course"));
    }

    #[gpui::test]
    fn cmd_enter_cannot_steer_a_mode_turn(cx: &mut TestAppContext) {
        assert!(!alternate_send(cx, "queue", "/plan sketch it first"));
    }

    #[gpui::test]
    fn text_left_in_the_composer_becomes_the_threads_draft(cx: &mut TestAppContext) {
        use std::time::Duration;
        let window = conversation(cx);
        let saved = |cx: &mut TestAppContext| {
            window
                .update(cx, |view, _, _| {
                    locked(&view.store)
                        .unwrap()
                        .composer_draft(view.summary.id)
                        .unwrap()
                })
                .unwrap()
        };
        let type_text = |cx: &mut TestAppContext, text: &str| {
            window
                .update(cx, |view, _, cx| {
                    view.input
                        .update(cx, |input, cx| input.set_text(text.into(), cx))
                })
                .unwrap();
            cx.run_until_parked();
        };
        type_text(cx, "half a thought");
        assert_eq!(saved(cx), None, "it waits for a pause in typing");
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert_eq!(saved(cx).unwrap().text, "half a thought");

        let (store, summary) = window
            .update(cx, |view, _, _| (view.store.clone(), view.summary.clone()))
            .unwrap();
        let reopened = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Conversation::new(summary, store, cx))
            })
            .unwrap()
        });
        reopened
            .update(cx, |view, _, cx| {
                let draft = locked(&view.store)
                    .unwrap()
                    .composer_draft(view.summary.id)
                    .unwrap()
                    .unwrap();
                view.restore_composer_draft(draft, cx);
            })
            .unwrap();
        cx.run_until_parked();
        reopened
            .update(cx, |view, _, cx| {
                assert_eq!(view.input.read(cx).content.as_ref(), "half a thought");
                assert!(view.composer_has_draft());
            })
            .unwrap();

        type_text(cx, "more");
        window
            .update(cx, |view, _, cx| view.flush_composer_draft(cx))
            .unwrap();
        assert_eq!(saved(cx).unwrap().text, "more", "leaving saves at once");
        type_text(cx, "");
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert_eq!(saved(cx), None, "an emptied composer clears the draft");
    }

    #[gpui::test]
    fn a_saved_draft_opens_in_the_composer_instead_of_the_thread(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, cx| {
                view.follow_ups.state.draft = Some(Message {
                    id: "app-prompt".into(),
                    text: "Review this project".into(),
                    attachments: vec!["/tmp/note.txt".into()],
                    ..Default::default()
                });
                view.adopt_saved_draft(cx);
                assert_eq!(view.input.read(cx).content.as_ref(), "Review this project");
                assert_eq!(
                    view.attachments,
                    [std::path::PathBuf::from("/tmp/note.txt")]
                );
                assert!(view.follow_ups.state.draft.is_none());
                assert!(view.has_unsent_draft());
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                let store = locked(&view.store).unwrap();
                assert!(store.follow_ups(view.summary.id).unwrap().draft.is_none());
                assert_eq!(
                    store.composer_draft(view.summary.id).unwrap().unwrap().text,
                    "Review this project"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn sending_a_saved_draft_preserves_the_new_composer(cx: &mut TestAppContext) {
        let window = conversation(cx);
        cx.update(|cx| {
            cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                prefs.follow_up = "steer".into()
            })
        });
        let (controls, received) = std::sync::mpsc::channel();
        window
            .update(cx, |view, _, cx| {
                view.follow_ups.state.draft = Some(Message {
                    id: "saved".into(),
                    text: "send later".into(),
                    ..Default::default()
                });
                view.busy = true;
                view.follow_ups.ready = true;
                view.controls = Some(controls);
                view.input
                    .update(cx, |i, cx| i.set_text("keep typing".into(), cx));
                view.attachments.push("/tmp/new-file.txt".into());
                view.draft_action(true, cx);
            })
            .unwrap();
        cx.run_until_parked();
        let control = received.try_recv().unwrap();
        let Decision::Steer(message) = control.decision else {
            panic!("expected saved draft");
        };
        assert_eq!(message.text, "send later");
        assert!(message.attachments.is_empty());
        window
            .update(cx, |view, _, cx| {
                assert_eq!(view.input.read(cx).content, "keep typing");
                assert_eq!(
                    view.attachments,
                    [std::path::PathBuf::from("/tmp/new-file.txt")]
                );
                assert!(view.follow_ups.state.draft.is_none());
            })
            .unwrap();
    }
}
