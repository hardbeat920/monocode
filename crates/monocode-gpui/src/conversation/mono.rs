use super::*;
use gpui::{App, EntityInputHandler};
use monocode_core::mono::{Message, Mono};
use std::path::PathBuf;
#[derive(Default)]
pub(super) struct ChatState {
    pub(super) pending: Vec<Message>,
    pub(super) emoji: bool,
    pub(super) delivered: std::collections::HashSet<String>,
    pub(super) sending: Option<String>,
    pub(super) failed: Vec<(String, String)>,
    pub(super) external: Option<(String, bool)>,
    /// ModelPicker with `hideSettings`: the model list opens on its own.
    pub(super) models: Option<(Entity<ModelList>, Vec<Subscription>)>,
    /// The setting pills' anchors, by `ComposerMenu::Setting` index.
    pub(super) setting_anchors: Vec<AnchorSlot>,
}
pub(crate) struct Submitted {
    pub text: String,
    pub attachments: Vec<PathBuf>,
}
pub(crate) struct QueueChanged {
    pub id: String,
    pub remove: bool,
}
impl EventEmitter<QueueChanged> for Conversation {}
impl EventEmitter<Submitted> for Conversation {}
impl Conversation {
    /// App launches are saved before installing a view. Reuse the normal durable
    /// draft/queue path instead of submitting an unrecorded seed prompt.
    pub(crate) fn install_app_launch(
        &mut self,
        settings: Settings,
        send: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.history_ready() {
            self.defer_history_action(
                move |this, cx| this.install_app_launch(settings, send, cx),
                cx,
            );
            return;
        }
        self.install_transfer(settings, None, cx);
        if send {
            self.draft_action(true, cx);
        }
        cx.emit(SessionChanged);
    }
    pub(crate) fn save_app_draft(&mut self, prompt: String, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_text(prompt, cx));
        self.save_message_draft(cx);
    }
    pub(crate) fn stop_app_session(&mut self, cx: &mut Context<Self>) {
        if self.needs_history_restore() {
            self.stop_if_busy(cx);
            self.defer_history_action(|this, cx| this.stop_app_session(cx), cx);
            return;
        }
        self.stop_if_busy(cx);
        self.follow_ups.state.paused = true;
        self.follow_ups.ready = false;
        self.follow_ups.requested = None;
        self.save_follow_ups(cx);
    }
    pub(crate) fn mono_quote(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() {
            return;
        }
        self.input.update(cx, |i, cx| {
            i.set_text(
                monocode_core::composer::append_selection_quote(&i.content, text),
                cx,
            );
        });
        window.focus(&self.input.focus_handle(cx), cx);
    }
    pub(crate) fn mono_external_pending(&self) -> bool {
        self.mono_ui.external.is_some()
    }
    pub(crate) fn mono_external_send(
        &mut self,
        prompt: String,
        draft: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.busy
            || self.stopping
            || self.mono_ui.external.is_some()
            || !self.input.read(cx).content.trim().is_empty()
        {
            return Err("Session is busy or has an unsent draft".into());
        }
        self.mono_ui.external = Some((prompt, draft));
        if self.history_ready() {
            self.mono_flush_external(cx);
        } else {
            self.load(cx);
        }
        cx.emit(SessionChanged);
        Ok(())
    }
    pub(super) fn mono_flush_external(&mut self, cx: &mut Context<Self>) {
        if !self.history_ready() || self.stopping {
            return;
        }
        if let Some((prompt, draft)) = self.mono_ui.external.take() {
            if draft {
                self.input.update(cx, |i, cx| i.set_text(prompt, cx));
            } else {
                self.install_transfer(self.settings.clone(), Some(prompt), cx);
            }
        }
    }
    pub(crate) fn mono_attention(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .child(self.approval_shelf(window, cx))
            .when_some(self.questions.front(), |d, (card, _)| {
                d.child(div().mb_2().child(card.clone()))
            })
            .into_any_element()
    }
    pub(crate) fn configure_mono(
        &mut self,
        mono: Mono,
        root: PathBuf,
        environment: std::collections::HashMap<String, String>,
        cx: &mut Context<Self>,
    ) {
        self.input
            .update(cx, |input, cx| input.mono_composer(&mono.label(), cx));
        let name = mono.label();
        self.transcript
            .update(cx, |t, _| t.mono_mode(&mono.mascot, mono.color, &name));
        self.settings.environment = environment;
        self.mono_ui.pending = mono.outbox.clone();
        self.mono_ui
            .delivered
            .retain(|id| mono.outbox.iter().any(|m| &m.id == id));
        self.sync_mono_pending(cx);
        self.settings.mono = Some(monocode_core::agent::MonoContext {
            mono,
            root,
            executable: std::env::current_exe().unwrap_or_default(),
        });
        cx.notify();
    }
    /// The user's queued messages show in the transcript as bubbles; the one
    /// just delivered is already a transcript row.
    pub(super) fn sync_mono_pending(&mut self, cx: &mut Context<Self>) {
        let pending = self
            .mono_ui
            .pending
            .iter()
            .filter(|m| !m.internal && !self.mono_ui.delivered.contains(&m.id))
            .map(|m| crate::transcript::PendingMessage {
                id: m.id.clone(),
                text: m.text.clone().into(),
                error: m.error.clone(),
                sent_at: m.sent_at,
                attachments: m.attachments.iter().map(PathBuf::from).collect(),
            })
            .collect();
        self.transcript
            .update(cx, |t, cx| t.set_pending(pending, cx));
    }
    pub(crate) fn mono_dispatch(&mut self, message: &Message, cx: &mut Context<Self>) -> bool {
        if self.mono_ui.delivered.contains(&message.id) || (self.busy && message.internal) {
            return false;
        }
        if !self.history_ready() {
            self.load(cx);
            return false;
        }
        if self.settings.pending_provider_switch.is_some() && self.busy {
            self.stop(cx);
        }
        if self.stopping
            || !self.loaded
            || self.settings.usage_limit.is_some()
            || self.account_switching
        {
            return false;
        }
        if self.busy {
            if self.mono_ui.sending.is_some()
                || !self.follow_ups.ready
                || !Kind::from_id(&self.summary.provider).can_steer()
                || self
                    .questions
                    .iter()
                    .any(|(card, _)| card.read(cx).blocks_steering())
            {
                return false;
            }
            let prepared = self.completion.read(cx).prepare_prompt(&message.text);
            let steer = monocode_core::follow_up::Message {
                id: message.id.clone(),
                text: message.text.clone(),
                sent_at: message.sent_at,
                prepared_prompt: (prepared != message.text).then_some(prepared),
                attachments: message.attachments.iter().map(PathBuf::from).collect(),
                steer: true,
                ..Default::default()
            };
            if self.controls.as_ref().is_some_and(|controls| {
                controls
                    .send(Control {
                        id: message.id.clone(),
                        decision: Decision::Steer(steer),
                    })
                    .is_ok()
            }) {
                self.mono_ui.sending = Some(message.id.clone());
                return true;
            }
            return false;
        }
        self.mono_ui.sending = None;
        self.settings.mono_message_id = Some(message.id.clone());
        self.settings.mono_sent_at = message.sent_at;
        self.settings.internal_message = message.internal;
        self.settings.attachments = message.attachments.iter().map(PathBuf::from).collect();
        self.launch_prompt(message.text.clone(), cx);
        true
    }
    pub(crate) fn mono_rows<'a>(&self, cx: &'a App) -> &'a [monocode_core::transcript::Row] {
        self.transcript.read(cx).source_rows()
    }
    pub(crate) fn mono_patch_card(
        &mut self,
        index: usize,
        id: &str,
        value: serde_json::Value,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.history_ready() || self.stopping {
            self.load(cx);
            return Err("Conversation history is loading; retry the update when ready".into());
        }
        let current = self
            .transcript
            .read(cx)
            .source_rows()
            .get(index)
            .filter(|r| r.role == "mono-card")
            .and_then(|r| serde_json::from_str::<serde_json::Value>(&r.text).ok());
        if current.as_ref().and_then(|v| v["id"].as_str()) != Some(id) {
            return Ok(());
        }
        let row = monocode_core::transcript::Row {
            role: "mono-card".into(),
            text: value.to_string(),
            first: true,
            sent_at: self.transcript.read(cx).source_rows()[index].sent_at,
        };
        locked(&self.store)?.write_rows(self.summary.id, index, std::slice::from_ref(&row))?;
        self.transcript
            .update(cx, |t, cx| t.patch(index, vec![row], cx));
        cx.notify();
        Ok(())
    }
    pub(crate) fn mono_status(&self) -> &str {
        if self.needs_approval() || self.settings.usage_limit.is_some() {
            "needs-you"
        } else if self.responding() {
            "working"
        } else {
            "idle"
        }
    }
    pub(crate) fn mono_model(&self) -> Option<String> {
        self.model.clone()
    }
    pub(crate) fn branch_name(&self) -> Option<&str> {
        self.branch.as_deref()
    }
    pub(crate) fn mono_model_label(&self) -> SharedString {
        self.model_label()
    }
    pub(crate) fn mono_stop(&mut self, cx: &mut Context<Self>) {
        self.stop_if_busy(cx);
    }
    pub(crate) fn mono_post(
        &mut self,
        role: &str,
        text: String,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.history_ready() {
            self.load(cx);
            return Err("Conversation history is loading; retry the report when ready".into());
        }
        if self.busy || self.stopping {
            return Err("Wait for the current turn before posting a report".into());
        }
        let mut posted = monocode_core::transcript::Transcript::default();
        if role == "assistant" {
            // A posted report is its own completed run. Its boundary keeps a
            // later report from hiding the previous notification's final reply.
            posted.begin("mono-system");
            posted.append("[Scheduled Mono report]");
        }
        posted.begin(role);
        if role == "mono-card" {
            // Card JSON is one metadata row, including large summaries.
            posted.rows[0].text = text;
        } else {
            // Match the store's row boundaries so the next post starts after
            // the complete report, even when it spans multiple display rows.
            posted.append(&text);
        }
        let at = self.transcript.read(cx).source_rows().len();
        locked(&self.store)?.write_rows(self.summary.id, at, &posted.rows)?;
        self.transcript
            .update(cx, |t, cx| t.patch(at, posted.rows, cx));
        self.summary.has_messages = true;
        cx.emit(SessionChanged);
        cx.notify();
        Ok(())
    }
    pub(crate) fn mono_access_options(&self) -> Vec<MenuItem> {
        let provider = Kind::from_id(&self.summary.provider);
        let mut items = vec![
            MenuAction::new("access:read-only", "Read-only")
                .icon("square-lock")
                .description("Inspect without changing files.")
                .checked(self.settings.access == AccessMode::ReadOnly)
                .into(),
        ];
        items.extend(ACCESS_MODES.iter().map(|(id, label, hint, glyph)| {
                    let hint = match (provider,*id) {
                        (Kind::Cursor,"supervised") => "Confirm commands using Cursor's permission policy; Cursor may accept edits automatically.",
                        (Kind::Pi | Kind::Omp,_) => "Tool confirmations follow this provider's configured extensions.",
                        _ => *hint,
                    };
                    let action = MenuAction::new(format!("access:{id}"), *label)
                        .icon(glyph)
                        .description(hint)
                        .checked(self.settings.access.id() == *id);
                    MenuItem::from(if *id == "full-access" {
                        action.icon_color(tint(AMBER_400, 0.9))
                    } else {
                        action
                    })
                }));
        if self.busy {
            items.push(MenuItem::Note("Changes apply to the next turn.".into()));
        }
        items
    }
    pub(crate) fn mono_access(
        &mut self,
        mode: monocode_core::agent::AccessMode,
        cx: &mut Context<Self>,
    ) {
        self.settings.access = mode;
        super::remember_access(mode, cx);
        self.save_settings(cx);
        cx.notify();
    }
    pub(crate) fn mono_delivered(&self) -> &std::collections::HashSet<String> {
        &self.mono_ui.delivered
    }
    pub(super) fn mono_message_delivered(&mut self, id: String, cx: &mut Context<Self>) {
        if self.mono_ui.sending.as_ref() == Some(&id) {
            self.mono_ui.sending = None;
        }
        self.mono_ui.delivered.insert(id);
        self.sync_mono_pending(cx);
    }
    pub(super) fn mono_message_failed(&mut self, id: String, error: String) {
        if self.mono_ui.sending.as_ref() == Some(&id) {
            self.mono_ui.sending = None;
        }
        self.mono_ui.failed.push((id, error));
    }
    pub(crate) fn mono_take_failures(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.mono_ui.failed)
    }
    pub(crate) fn mono_model_options(&self) -> Vec<MenuItem> {
        let mut items: Vec<MenuAction> = Vec::new();
        if self.model_info().is_some_and(|m| !m.efforts.is_empty()) {
            items.push(
                MenuAction::new("reasoning", self.effort_setting_label())
                    .value(self.effort_label())
                    .submenu(self.effort_items()),
            );
        }
        if self.model_info().is_some_and(|m| m.fast) {
            items.push(MenuAction::new("fast", "Fast").toggle(self.settings.fast));
        }
        if let Some(model) = self.model_info().filter(|m| !m.tiers.is_empty()) {
            let mut tiers =
                vec![MenuAction::new("tier:", "Standard").checked(self.selected_tier().is_none())];
            tiers.extend(model.tiers.iter().map(|(id, name)| {
                MenuAction::new(format!("tier:{id}"), name.clone())
                    .checked(self.selected_tier() == Some(id.as_str()))
            }));
            items.push(
                MenuAction::new("service-tier", "Service Tier")
                    .value(
                        self.selected_tier()
                            .and_then(|id| model.tiers.iter().find(|(key, _)| key == id))
                            .map(|(_, label)| label.as_str())
                            .unwrap_or_else(|| self.selected_tier().unwrap_or("Standard"))
                            .to_owned(),
                    )
                    .submenu(tiers),
            );
        }
        if self.model_info().is_some_and(|m| m.thinking) {
            items.push(
                MenuAction::new("thinking", "Thinking")
                    .toggle(self.settings.thinking.unwrap_or(false)),
            );
        }
        if let Some(model) = self.model_info() {
            for control in &model.controls {
                if Kind::from_id(&self.summary.provider) == Kind::OpenCode && control.id == "agent"
                {
                    continue;
                }
                let selected = self
                    .settings
                    .model_settings
                    .get(&control.id)
                    .unwrap_or(&control.current);
                let label = match control.id.as_str() {
                    "effort" | "reasoning" => "Effort",
                    _ => &control.name,
                };
                if model_control_toggle(control) {
                    items.push(
                        MenuAction::new(format!("model-toggle:{}", control.id), label.to_owned())
                            .toggle(selected == "true"),
                    );
                    continue;
                }
                let choices = control
                    .choices
                    .iter()
                    .map(|(value, name)| {
                        let key =
                            serde_json::to_string(&(control.id.clone(), value.clone())).unwrap();
                        MenuAction::new(format!("model-setting:{key}"), name.clone())
                            .checked(selected == value)
                    })
                    .collect();
                items.push(
                    MenuAction::new(format!("model-control:{}", control.id), label.to_owned())
                        .value(
                            control
                                .choices
                                .iter()
                                .find(|(value, _)| value == selected)
                                .map(|(_, name)| name)
                                .unwrap_or(selected)
                                .clone(),
                        )
                        .submenu(choices),
                );
            }
        }
        items.sort_by_key(|action| {
            let key = action
                .id
                .strip_prefix("model-control:")
                .or_else(|| action.id.strip_prefix("model-toggle:"))
                .unwrap_or(&action.id);
            match key {
                "fast" => 0,
                "lead" => 1,
                "effort" => 2,
                "sidekick" => 3,
                "reasoning" if Kind::from_id(&self.summary.provider) == Kind::Codex => 5,
                "reasoning" => 4,
                "reasoningEffort" | "reasoning_effort" => 5,
                "service-tier" | "serviceTier" => 6,
                "thinking" => 7,
                "variant" => 8,
                "agent" => 9,
                "context" => 10,
                _ => 99,
            }
        });
        items.into_iter().map(MenuItem::from).collect()
    }
    pub(crate) fn mono_pick_model_option(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.busy || self.stopping {
            return Err("Wait for the current reply before changing model settings".into());
        }
        self.menu_choice(id, cx);
        cx.emit(SessionChanged);
        Ok(())
    }
    pub(crate) fn mono_config(
        &mut self,
        provider: Kind,
        model: Option<String>,
        values: std::collections::HashMap<String, String>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.busy || self.stopping {
            return Err("Wait for the current reply before changing models".into());
        }
        if self.summary.provider != provider.id() {
            self.summary.provider = provider.id().into();
            self.summary.provider_id = None;
            self.catalog.clear();
        }
        self.model = model;
        self.settings.effort = super::preferred_effort(provider.id(), cx);
        self.settings.fast = false;
        self.settings.thinking = None;
        self.settings.service_tier = None;
        if let Some(context) = &mut self.context {
            context.window = None;
        }
        self.settings.model_settings = values;
        self.resolved_model = None;
        self.refresh_catalog(cx);
        self.save_settings(cx);
        cx.emit(SessionChanged);
        cx.notify();
        Ok(())
    }
    pub(super) fn render_mono(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mono = self.settings.mono.as_ref().unwrap().mono.clone();
        let status = self.mono_status();
        let name = mono.label();
        // MonoHeader's greeting: it shows only before the first message.
        let body = if !self.summary.has_messages {
            let greeting = div().flex().flex_col().items_center().justify_center().px_6().py_12()
                .child(crate::monos::avatar::avatar(&mono.mascot, mono.color, status, 56., false))
                .child(div().mt(px(-8.)).h(px(32.)).px(px(14.)).flex().items_center().rounded_full().border_1().border_color(content(0.14)).bg(content(0.06)).text_size(px(13.)).font_weight(gpui::FontWeight::SEMIBOLD).child(name))
                .child(div().mt_3().max_w(px(384.)).text_center().text_size(px(13.)).line_height(px(21.)).text_color(content(0.5)).child(if mono.projects.is_empty(){"Add the projects I should work on in my settings, or just ask me something.".into()}else{format!("I can see every session, note and folder in {}. Ask what’s going on, or hand me work.",mono.projects.iter().map(|p|std::path::Path::new(p).file_name().unwrap_or_default().to_string_lossy()).collect::<Vec<_>>().join(", "))}));
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(greeting)
                .into_any_element()
        } else {
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    self.transcript
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                )
                .into_any_element()
        };
        self.attachment_drop_target("mono-chat", cx)
            .capture_action(cx.listener(|this, _: &input::Paste, _, cx| {
                if this.paste_attachments(cx) {
                    cx.stop_propagation();
                }
            }))
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .when_some(self.menu.as_ref(), |d, menu| {
                d.child(
                    menu_popover(&menu.menu, Anchor::Slot(self.anchor(menu.kind).clone()), cx)
                        .side(Side::Top)
                        .when(menu.kind == ComposerMenu::Account, |p| {
                            p.width(250.).max_height(260.)
                        }),
                )
            })
            .children(self.model_list_popover(cx))
            .child(body)
            .child(
                div()
                    .w_full()
                    .max_w(px(COMPOSER_COLUMN))
                    .mx_auto()
                    .px(px(10.))
                    .pb_1p5()
                    .flex()
                    .flex_col()
                    .children(
                        self.mono_ui
                            .pending
                            .iter()
                            .filter(|m| m.internal && m.error.is_some())
                            .map(|message| {
                                let retry = message.id.clone();
                                let remove = retry.clone();
                                div()
                                    .id(SharedString::from(format!("mono-queued-{}", message.id)))
                                    .debug_selector(|| "mono-internal-queue".into())
                                    .mb_1()
                                    .flex()
                                    .justify_end()
                                    .child(
                                        div()
                                            .max_w(px(440.))
                                            .px_3()
                                            .py_2()
                                            .rounded_xl()
                                            .border_1()
                                            .border_color(content(0.15))
                                            .bg(content(0.04))
                                            .text_size(px(13.))
                                            .text_color(content(0.6))
                                            .child(if message.internal {
                                                "Completion update".into()
                                            } else {
                                                message.text.clone()
                                            })
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .text_color(content(0.4))
                                                    .child(
                                                        message
                                                            .error
                                                            .clone()
                                                            .unwrap_or_else(|| "Pending".into()),
                                                    ),
                                            )
                                            .when(message.error.is_some(), |d| {
                                                d.child(
                                                    div()
                                                        .flex()
                                                        .gap_2()
                                                        .child(
                                                            icon_button(
                                                                "mono-queue-retry",
                                                                "Retry queued message",
                                                                "refresh-cw",
                                                            )
                                                            .on_click(cx.listener(
                                                                move |_, _, _, cx| {
                                                                    cx.emit(QueueChanged {
                                                                        id: retry.clone(),
                                                                        remove: false,
                                                                    })
                                                                },
                                                            )),
                                                        )
                                                        .child(
                                                            icon_button(
                                                                "mono-queue-discard",
                                                                "Discard queued message",
                                                                "close",
                                                            )
                                                            .on_click(cx.listener(
                                                                move |_, _, _, cx| {
                                                                    cx.emit(QueueChanged {
                                                                        id: remove.clone(),
                                                                        remove: true,
                                                                    })
                                                                },
                                                            )),
                                                        ),
                                                )
                                            }),
                                    )
                            }),
                    )
                    .children(self.usage_notice(true, cx))
                    .children(self.auth_notice(cx))
                    .child(self.approval_shelf(window, cx))
                    .child({
                        let composer = div()
                            .child(self.mono_composer_box(window, cx))
                            .child(self.mono_composer_tools(cx))
                            .into_any_element();
                        let focus = self.input.focus_handle(cx);
                        let card = self.questions.front().map(|(card, _)| card);
                        self.question_overlay
                            .wrap(composer, card, &focus, window, cx)
                    }),
            )
            .into_any_element()
    }

    /// Whether the model list and a composer menu are open.
    #[cfg(test)]
    pub(crate) fn mono_menus_open(&self) -> (bool, bool) {
        (self.mono_ui.models.is_some(), self.menu.is_some())
    }
    /// `ModelPicker` with `hideSettings`: its trigger opens the model list.
    pub(super) fn toggle_mono_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mono_ui.models.is_some() {
            self.close_mono_models(window, cx);
            return;
        }
        self.menu = None;
        self.completion
            .update(cx, |completion, cx| completion.dismiss(cx));
        self.git_picker = None;
        self.git_events = None;
        let provider = Kind::from_id(&self.summary.provider);
        let list = cx.new(|cx| {
            let hidden = cx
                .try_global::<crate::settings::Preferences>()
                .map(|p| p.hidden_for(&self.summary.cwd))
                .unwrap_or_default();
            ModelList::new_for(
                provider,
                self.model.clone(),
                &self.summary.cwd,
                self.settings.account.clone(),
                cx,
            )
            .hide_providers(hidden)
            .allow_provider_switch(true)
        });
        let events = vec![cx.subscribe_in(
            &list,
            window,
            |this, _, picked: &ModelPicked, window, cx| {
                if this.apply_model_pick(picked, cx) {
                    this.close_mono_models(window, cx);
                }
            },
        )];
        window.focus(&list.read(cx).search_focus(cx), cx);
        self.mono_ui.models = Some((list, events));
        cx.notify();
    }
    pub(super) fn model_list_popover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (list, _) = self.mono_ui.models.as_ref()?;
        let this = cx.entity().downgrade();
        Some(
            popover("composer-models", Anchor::Slot(self.model_anchor.clone()))
                .side(Side::Top)
                .align(Align::End)
                .width(310.)
                .height(442.)
                .on_dismiss(move |window, cx| {
                    let _ = this.update(cx, |this, cx| this.close_mono_models(window, cx));
                })
                .child(list.clone())
                .into_any_element(),
        )
    }
    fn close_mono_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mono_ui.models = None;
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }
    /// ModelControlPills' select menus: effort (with fast and service tier
    /// grouped under it), then each provider control.
    pub(super) fn mono_setting_menu(
        &self,
        ix: usize,
    ) -> (&'static str, Vec<MenuItem>, f32, MenuStyle) {
        let Some(model) = self.model_info() else {
            return ("Model setting", Vec::new(), 210., MenuStyle::Explorer);
        };
        let mut items = Vec::new();
        if ix == 0 {
            let group = |items: &mut Vec<MenuItem>, label: &str, rows: Vec<MenuAction>| {
                if !items.is_empty() {
                    items.push(MenuItem::Separator);
                }
                items.push(MenuItem::Header(label.to_owned().into()));
                items.extend(rows.into_iter().map(MenuItem::from));
            };
            if !model.efforts.is_empty() {
                group(&mut items, self.effort_setting_label(), self.effort_items());
            }
            if model.fast {
                group(
                    &mut items,
                    "Fast",
                    vec![
                        MenuAction::new("fast:on", "On").checked(self.settings.fast),
                        MenuAction::new("fast:off", "Off").checked(!self.settings.fast),
                    ],
                );
            }
            if !model.tiers.is_empty() {
                let mut tiers = vec![
                    MenuAction::new("tier:", "Standard").checked(self.selected_tier().is_none()),
                ];
                tiers.extend(model.tiers.iter().map(|(id, name)| {
                    MenuAction::new(format!("tier:{id}"), name.clone())
                        .checked(self.selected_tier() == Some(id.as_str()))
                }));
                group(&mut items, "Service Tier", tiers);
            }
            if items
                .iter()
                .filter(|i| matches!(i, MenuItem::Header(_)))
                .count()
                == 1
            {
                items.retain(|i| !matches!(i, MenuItem::Header(_)));
            }
            return ("Effort", items, 210., MenuStyle::Explorer);
        }
        if let Some(control) = model.controls.get(ix - 1) {
            let selected = self
                .settings
                .model_settings
                .get(&control.id)
                .unwrap_or(&control.current);
            items.extend(control.choices.iter().map(|(value, name)| {
                let key = serde_json::to_string(&(control.id.clone(), value.clone())).unwrap();
                MenuItem::from(
                    MenuAction::new(format!("model-setting:{key}"), name.clone())
                        .checked(selected == value),
                )
            }));
        }
        ("Model setting", items, 210., MenuStyle::Explorer)
    }
    /// The form's box: the field and send, as `MonoComposer` lays them out
    /// when it has controls.
    fn mono_composer_box(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let runner = self.composer_runner(cx);
        let focused = self.input.focus_handle(cx).contains_focused(window, cx);
        let ready = self.loaded
            && !self.attaching
            && (!self.input.read(cx).content.trim().is_empty() || !self.attachments.is_empty());
        let border = if self.file_drag {
            accent_color().opacity(0.6)
        } else {
            content(if focused { 0.2 } else { 0.1 })
        };
        div()
            .id("mono-composer")
            .debug_selector(|| "mono-composer".into())
            .relative()
            .p(px(1.))
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.input.focus_handle(cx), cx)),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        // One line is 18px plus 8px field padding, 10px row
                        // padding and 2px border. Decide after text wraps.
                        let radius = if bounds.size.height <= px(38.) {
                            bounds.size.height / 2.
                        } else {
                            px(COMPOSER_RADIUS)
                        };
                        window.paint_quad(
                            gpui::fill(bounds, content(0.03))
                                .corner_radii(radius)
                                .border_widths(px(1.))
                                .border_color(border),
                        );
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(self.composer_anchor.probe())
            .children(runner)
            .children(self.attachment_chips(cx))
            .child(
                // Even padding centres send beside one line, and keeps it at
                // the bottom as the text grows.
                div()
                    .flex()
                    .items_end()
                    .gap_1p5()
                    .p(px(5.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .self_center()
                            .px_1p5()
                            .py_1()
                            .child(self.input.clone()),
                    )
                    .child(
                        div()
                            .id("mono-send")
                            .debug_selector(|| "mono-send".into())
                            .role(Role::Button)
                            .aria_label("Send")
                            .flex_shrink_0()
                            .size(px(26.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(white(if ready { 1. } else { 0.3 }))
                            .when(ready, |d| d.hover(|s| s.bg(white(0.9))))
                            .child(icon("mono-arrow-up", 14.).text_color(black(if ready {
                                1.
                            } else {
                                0.4
                            })))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if ready {
                                    this.send(cx)
                                }
                            })),
                    ),
            )
            .children(self.attachment_drop_overlay(8.))
            .into_any_element()
    }
    pub(super) fn model_controls(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let text_control = |id: SharedString, open: bool| {
            div()
                .id(id)
                .role(Role::Button)
                .flex_shrink_0()
                .h(px(26.))
                .min_w_0()
                .px(px(10.))
                .gap_1p5()
                .rounded_full()
                .flex()
                .items_center()
                .text_size(px(12.))
                .text_color(content(if open { 1. } else { 0.55 }))
                .when(open, |d| d.bg(content(0.08)))
                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        let model = self.model_info().cloned();
        let mut pills: Vec<AnyElement> = Vec::new();
        if let Some(model) = &model {
            let anchors = 1 + model.controls.len();
            if self.mono_ui.setting_anchors.len() < anchors {
                self.mono_ui
                    .setting_anchors
                    .resize_with(anchors, AnchorSlot::default);
            }
            let grouped = !model.efforts.is_empty() || !model.tiers.is_empty();
            if grouped {
                let open = self.menu_is(ComposerMenu::Setting(0));
                let (glyph, label, value) = if model.efforts.is_empty() {
                    (
                        "zap",
                        "Service Tier",
                        self.selected_tier()
                            .and_then(|id| model.tiers.iter().find(|(t, _)| t == id))
                            .map_or("Standard".into(), |(_, name)| name.clone()),
                    )
                } else {
                    (
                        "gauge",
                        self.effort_setting_label(),
                        self.effort_label().to_string(),
                    )
                };
                pills.push(
                    text_control("mono-effort".into(), open)
                        .max_w(px(112.))
                        .aria_label(format!("{label}: {value}"))
                        .aria_expanded(open)
                        .child(self.mono_ui.setting_anchors[0].probe())
                        .child(icon(glyph, 14.).text_color(content(if open { 1. } else { 0.55 })))
                        .child(div().min_w_0().truncate().child(value))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_menu(ComposerMenu::Setting(0), window, cx)
                        }))
                        .into_any_element(),
                );
            }
            if model.fast && !grouped {
                let on = self.settings.fast;
                pills.push(
                    text_control("mono-fast".into(), false)
                        .aria_label(format!("Fast mode: {}", if on { "On" } else { "Off" }))
                        .child(div().when(!on, |d| d.opacity(0.5)).child("Fast"))
                        .on_click(cx.listener(|this, _, _, cx| this.menu_choice("fast", cx)))
                        .into_any_element(),
                );
            }
            if model.thinking {
                let on = self.settings.thinking.unwrap_or(false);
                pills.push(
                    text_control("mono-thinking".into(), false)
                        .aria_label(format!("Thinking: {}", if on { "On" } else { "Off" }))
                        .child(div().when(!on, |d| d.opacity(0.5)).child("Thinking"))
                        .on_click(cx.listener(|this, _, _, cx| this.menu_choice("thinking", cx)))
                        .into_any_element(),
                );
            }
            for (ix, control) in model.controls.iter().enumerate() {
                if Kind::from_id(&self.summary.provider) == Kind::OpenCode && control.id == "agent"
                {
                    continue;
                }
                let kind = ComposerMenu::Setting(ix + 1);
                let toggle = model_control_toggle(control);
                let open = !toggle && self.menu_is(kind);
                let selected = self
                    .settings
                    .model_settings
                    .get(&control.id)
                    .unwrap_or(&control.current);
                let value = control
                    .choices
                    .iter()
                    .find(|(value, _)| value == selected)
                    .map_or(selected.clone(), |(_, name)| name.clone());
                let toggle_id = format!("model-toggle:{}", control.id);
                let on = selected == "true";
                let title = if toggle {
                    format!("{}: {}", control.name, if on { "On" } else { "Off" })
                } else {
                    format!("{}: {value}", control.name)
                };
                pills.push(
                    text_control(format!("mono-control-{ix}").into(), open)
                        .max_w(px(112.))
                        .aria_label(title)
                        .when(!toggle, |d| {
                            d.aria_expanded(open)
                                .child(self.mono_ui.setting_anchors[ix + 1].probe())
                        })
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .when(toggle && !on, |d| d.opacity(0.5))
                                .child(if toggle { control.name.clone() } else { value }),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if toggle {
                                this.menu_choice(&toggle_id, cx);
                            } else {
                                this.toggle_menu(kind, window, cx);
                            }
                        }))
                        .into_any_element(),
                );
            }
        }
        pills
    }
    /// Under the box: attach and emoji on the left, the model, its settings
    /// and the context meter on the right, the last lined up under send.
    fn mono_composer_tools(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tool = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .group(id)
                .flex_shrink_0()
                .size(px(26.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(content(0.08)))
        };
        let text_control = |id: SharedString, open: bool| {
            div()
                .id(id)
                .role(Role::Button)
                .flex_shrink_0()
                .h(px(26.))
                .min_w_0()
                .px(px(10.))
                .gap_1p5()
                .rounded_full()
                .flex()
                .items_center()
                .text_size(px(12.))
                .text_color(content(if open { 1. } else { 0.55 }))
                .when(open, |d| d.bg(content(0.08)))
                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        let pills = self.model_controls(cx);
        let models_open = self.mono_ui.models.is_some();
        let ratio = self.context.and_then(|usage| usage.ratio());
        let headline = ratio
            .map(|r| format!("{:.0}% context used", r * 100.))
            .unwrap_or_else(|| "No context used yet".into());
        let detail = self.context.map(|usage| {
            format!(
                "{} / {} tokens",
                monocode_core::context::format_tokens(usage.used),
                monocode_core::context::format_tokens(usage.window.unwrap_or(0))
            )
        });
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(2.))
            .pt_1()
            .pl_1()
            .pr_1p5()
            .child(
                tool("mono-attach", "Attach files")
                    .debug_selector(|| "mono-attach".into())
                    .child(
                        icon("mono-attach", 17.)
                            .text_color(content(0.55))
                            .group_hover("mono-attach", |s| s.text_color(content(1.))),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.choose_attachments(cx))),
            )
            .child(
                tool("mono-emoji", "Add emoji")
                    .when(self.mono_ui.emoji, |d| d.bg(content(0.08)))
                    .child(
                        icon("smile", 13.)
                            .text_color(content(if self.mono_ui.emoji { 1. } else { 0.55 }))
                            .group_hover("mono-emoji", |s| s.text_color(content(1.))),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        if cfg!(target_os = "macos") {
                            window.focus(&this.input.focus_handle(cx), cx);
                            window.show_character_palette();
                            return;
                        }
                        this.mono_ui.emoji = !this.mono_ui.emoji;
                        cx.notify();
                    })),
            )
            .when(self.mono_ui.emoji, |d| {
                d.child(
                    div().flex().gap_1().children(
                        ["👍", "❤️", "🙂", "🎉", "👀", "✨", "🙏", "🚀"]
                            .into_iter()
                            .enumerate()
                            .map(|(ix, emoji)| {
                                div()
                                    .id(("mono-emoji", ix))
                                    .text_size(px(18.))
                                    .rounded_md()
                                    .hover(|s| s.bg(content(0.1)))
                                    .child(emoji)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.input.update(cx, |i, cx| {
                                            i.replace_text_in_range(None, emoji, window, cx)
                                        });
                                        this.mono_ui.emoji = false;
                                        cx.notify();
                                    }))
                            }),
                    ),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        text_control("mono-model".into(), models_open)
                            .min_w_0()
                            .max_w(px(220.))
                            .flex_shrink(1.)
                            .debug_selector(|| "mono-model".into())
                            .aria_label(format!(
                                "{}, {}",
                                Kind::from_id(&self.summary.provider).label(),
                                self.model_label()
                            ))
                            .aria_expanded(models_open)
                            .child(self.model_anchor.probe())
                            .child(provider_icon(&self.summary.provider, 14.))
                            .child(div().min_w_0().truncate().child(self.model_label()))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_mono_models(window, cx)
                            })),
                    )
                    .children(pills)
                    .child(
                        // A Mono is one long conversation; this is how full its
                        // current thread is. It rotates threads itself.
                        div()
                            .id("mono-context-meter")
                            .role(Role::Image)
                            .aria_label(format!(
                                "{headline}{}",
                                detail
                                    .as_ref()
                                    .map(|s| format!(", {s}"))
                                    .unwrap_or_default()
                            ))
                            .flex_shrink_0()
                            .size(px(26.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(content(0.08)))
                            .child(self.context_anchor.probe())
                            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                this.context_hovered = *hovered;
                                cx.notify();
                            }))
                            .child(crate::context_meter::ring(ratio.unwrap_or(0.)))
                            .when(self.context_hovered, |d| {
                                d.child(
                                    popover(
                                        "mono-context-tooltip",
                                        Anchor::Slot(self.context_anchor.clone()),
                                    )
                                    .side(Side::Top)
                                    .align(Align::End)
                                    .max_width(240.)
                                    .padding(10.)
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(content(1.))
                                            .child(headline)
                                            .when_some(detail, |d, detail| {
                                                d.child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .text_color(content(0.5))
                                                        .child(detail),
                                                )
                                            }),
                                    ),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use monocode_core::{store::Store, transcript::Transcript};
    use serde_json::json;
    use std::sync::Mutex;

    #[gpui::test]
    fn long_reports_preserve_card_and_follow_up_positions(cx: &mut TestAppContext) {
        for report in [
            "x".repeat(monocode_core::transcript::ROW_BYTES + 1),
            "A finding\n".repeat(26),
            "**Habit report**\n\n🙂 café 漢字\n\n".repeat(100),
        ] {
            let mut store = Store::open_monocode(Path::new(":memory:")).unwrap();
            let session = store.create("codex", "/project").unwrap();
            let mut history = Transcript::default();
            history.begin("user");
            history.append("Earlier question");
            history.begin("assistant");
            history.append("Earlier reply");
            store.write_rows(session.id, 0, &history.rows).unwrap();
            let chat = cx.new(|cx| Conversation::new(session, Arc::new(Mutex::new(store)), cx));
            chat.update(cx, |chat, cx| {
                chat.loaded = true;
                chat.transcript
                    .update(cx, |view, cx| view.patch(0, history.rows.clone(), cx));
                chat.mono_post("assistant", report.clone(), cx).unwrap();
                let card_index = chat.mono_rows(cx).len();
                let mut card = json!({
                    "id":"report-card", "type":"document", "documentId":"report-document",
                    "title":"Bug triage", "summary":"Detailed findings 🙂. ".repeat(200),
                });
                chat.mono_post("mono-card", card.to_string(), cx).unwrap();
                card["title"] = json!("Updated bug triage");
                chat.mono_patch_card(card_index, "report-card", card.clone(), cx)
                    .unwrap();
                chat.mono_post("assistant", "Another report".into(), cx)
                    .unwrap();

                let rows = chat.mono_rows(cx);
                assert_eq!(&rows[..history.rows.len()], history.rows);
                assert_eq!(rows[history.rows.len()].role, "mono-system");
                assert_eq!(
                    rows[history.rows.len()..card_index]
                        .iter()
                        .filter(|row| row.role == "assistant")
                        .map(|row| row.text.as_str())
                        .collect::<String>(),
                    report,
                );
                assert!(
                    rows.iter()
                        .filter(|row| row.role == "assistant")
                        .all(|row| {
                            row.text.len() <= monocode_core::transcript::ROW_BYTES
                                && row.text.matches('\n').count() <= 12
                        })
                );
                assert_eq!(rows.iter().filter(|row| row.role == "mono-card").count(), 1);
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&rows[card_index].text).unwrap(),
                    card,
                );
                assert_eq!(rows.last().unwrap().text, "Another report");
                let mut reloaded = locked(&chat.store)
                    .unwrap()
                    .load(chat.summary.id)
                    .unwrap()
                    .rows;
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&reloaded[card_index].text).unwrap(),
                    card,
                );
                // Restoring the card id can change JSON object field order.
                reloaded[card_index].text = rows[card_index].text.clone();
                assert_eq!(
                    reloaded, rows,
                    "reloading keeps the displayed rows and their message boundaries",
                );
            });
        }
    }
}
