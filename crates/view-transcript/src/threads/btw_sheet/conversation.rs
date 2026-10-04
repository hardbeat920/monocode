//! Port of `useBtwConversation` in src/features/sessions/ui/BtwSheet.tsx:
//! the state behind `/btw` side conversations. The sheet shows one tab per
//! side thread in the session, plus unsent drafts, and asks the active tab's
//! questions through [`BtwHost`].
//!
//! React state becomes fields, the memos become methods, and the effects
//! run in [`BtwConversation::set_props`].

use std::collections::HashMap;
use std::sync::Arc;

use gpui::App;
use monocode_core::block::{
    Block, BlockRole, BtwMessage, BtwMessageRole, BtwThread, BtwThreadStatus, ModelSettings,
};
use monocode_core::btw::BtwSessionThread;
use monocode_core::transcript::BlockRef;
use monocode_core::transcript::activity::group_turns;
use monocode_core::{Extra, HarnessId};

use super::host::{BtwHost, BtwRequest};
use crate::threads::parts::now_ms;

/// The hook's options that are data.
#[derive(Clone, Debug, PartialEq)]
pub struct BtwConversationProps {
    /// False when this session cannot take side questions right now.
    pub available: bool,
    pub blocks: Arc<Vec<Block>>,
    /// The session's current harness; each tab resolves its own from its turn.
    pub harness: HarnessId,
    pub managed: bool,
    pub model: String,
    pub model_settings: ModelSettings,
}

impl Default for BtwConversationProps {
    fn default() -> Self {
        Self {
            available: false,
            blocks: Arc::default(),
            harness: HarnessId::Claude,
            managed: false,
            model: String::new(),
            model_settings: ModelSettings::new(),
        }
    }
}

/// `BtwTab`: one tab in the sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct BtwTab {
    pub id: String,
    pub turn: Vec<Block>,
    pub thread: Option<BtwThread>,
    pub question: Option<String>,
    pub status: Option<BtwThreadStatus>,
}

/// Unsent text the side composer starts from. A new key makes a new
/// composer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seed {
    pub key: u64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Draft {
    id: String,
    turn_id: String,
}

/// The conversation state.
#[derive(Default)]
pub struct BtwConversation {
    props: BtwConversationProps,
    turns: Vec<Vec<Block>>,
    entries: Vec<BtwSessionThread>,
    requested_open: bool,
    /// Stays true while the close animation plays.
    rendered: bool,
    drafts: Vec<Draft>,
    draft_texts: HashMap<String, String>,
    active_id: Option<String>,
    draft_model: Option<String>,
    draft_model_settings: Option<ModelSettings>,
    optimistic: Option<(String, BtwMessage)>,
    seed: Seed,
}

/// `compactQuestion`: a tab's label.
pub fn compact_question(text: Option<&str>) -> String {
    let compact = text
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "New question".into());
    if monocode_core::js::len(&compact) > 48 {
        format!("{}…", monocode_core::js::slice_prefix(&compact, 45))
    } else {
        compact
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl BtwConversation {
    pub fn new() -> Self {
        Self::default()
    }

    /// New props, and the effects that watched them.
    pub fn set_props(&mut self, props: BtwConversationProps, host: &dyn BtwHost) {
        let blocks_changed =
            !Arc::ptr_eq(&props.blocks, &self.props.blocks) || props.managed != self.props.managed;
        self.props = props;
        if blocks_changed {
            let refs: Vec<BlockRef> = self.props.blocks.iter().cloned().map(Arc::new).collect();
            self.turns = group_turns(&refs, self.props.managed)
                .into_iter()
                .map(|turn| turn.into_iter().map(Arc::unwrap_or_clone).collect())
                .collect();
            self.entries = host.session_threads(&self.props.blocks, self.props.managed);
        }
        // `[available]`: a session that cannot ask closes the sheet.
        if !self.props.available {
            self.requested_open = false;
        }
        // `[optimistic, entries]`: the saved thread caught up.
        if let Some((thread_id, message)) = &self.optimistic
            && self.entries.iter().any(|entry| {
                &entry.thread.id == thread_id
                    && entry
                        .thread
                        .messages
                        .iter()
                        .any(|saved| saved.id == message.id)
            })
        {
            self.optimistic = None;
        }
        // `[entries]`: drafts that became saved threads.
        let entries = &self.entries;
        self.drafts
            .retain(|draft| !entries.iter().any(|entry| entry.thread.id == draft.id));
        self.sync_rendered();
    }

    pub fn props(&self) -> &BtwConversationProps {
        &self.props
    }

    fn sync_rendered(&mut self) {
        if self.open() && !self.rendered {
            self.rendered = true;
        }
    }

    /// The sheet is open.
    pub fn open(&self) -> bool {
        self.requested_open && self.props.available
    }

    /// The sheet is on screen, open or closing.
    pub fn rendered(&self) -> bool {
        self.rendered
    }

    pub fn seed(&self) -> &Seed {
        &self.seed
    }

    /// One tab per side thread, oldest first, then the unsent drafts.
    pub fn tabs(&self) -> Vec<BtwTab> {
        let mut next: Vec<BtwTab> = self
            .entries
            .iter()
            .map(|entry| BtwTab {
                id: entry.thread.id.clone(),
                turn: entry.turn.clone(),
                thread: Some(entry.thread.clone()),
                question: None,
                status: None,
            })
            .collect();
        for draft in &self.drafts {
            if next.iter().any(|tab| tab.id == draft.id) {
                continue;
            }
            if let Some(turn) = self
                .turns
                .iter()
                .find(|turn| turn.first().is_some_and(|block| block.id == draft.turn_id))
            {
                next.push(BtwTab {
                    id: draft.id.clone(),
                    turn: turn.clone(),
                    thread: None,
                    question: None,
                    status: None,
                });
            }
        }
        for tab in &mut next {
            let pending = self
                .optimistic
                .as_ref()
                .filter(|(thread_id, _)| thread_id == &tab.id)
                .map(|(_, message)| message);
            tab.question = tab
                .thread
                .as_ref()
                .and_then(|thread| {
                    thread
                        .messages
                        .iter()
                        .find(|message| message.role == BtwMessageRole::User)
                })
                .map(|message| message.text.clone())
                .or_else(|| pending.map(|message| message.text.clone()));
            tab.status = tab
                .thread
                .as_ref()
                .map(|thread| thread.status)
                .or_else(|| pending.map(|_| BtwThreadStatus::Running));
        }
        next
    }

    /// The active tab: the chosen one, else the newest.
    pub fn active(&self) -> Option<BtwTab> {
        let tabs = self.tabs();
        let chosen = self
            .active_id
            .as_ref()
            .and_then(|id| tabs.iter().find(|tab| &tab.id == id).cloned());
        chosen.or_else(|| tabs.last().cloned())
    }

    pub fn active_tab_id(&self) -> Option<String> {
        self.active().map(|tab| tab.id)
    }

    /// The active tab's saved thread.
    pub fn persisted(&self) -> Option<BtwThread> {
        self.active().and_then(|tab| tab.thread)
    }

    fn tab_harness_for(&self, tab: &BtwTab, host: &dyn BtwHost) -> Option<HarnessId> {
        if let Some(harness) = tab.thread.as_ref().and_then(|thread| thread.harness) {
            return Some(harness);
        }
        let threads = tab
            .turn
            .iter()
            .find(|block| block.role == BlockRole::User)
            .and_then(|block| block.btw_threads.as_deref());
        host.surface_harness(&self.props.blocks, &tab.turn, self.props.harness, threads)
    }

    /// The active tab's own harness, when it can take questions.
    pub fn tab_harness(&self, host: &dyn BtwHost) -> Option<HarnessId> {
        self.active()
            .and_then(|tab| self.tab_harness_for(&tab, host))
    }

    /// The side composer's harness.
    pub fn harness(&self, host: &dyn BtwHost) -> HarnessId {
        self.tab_harness(host).unwrap_or(self.props.harness)
    }

    /// The side composer can ask.
    pub fn can_ask(&self, host: &dyn BtwHost) -> bool {
        self.tab_harness(host).is_some()
    }

    fn base_model_for(&self, tab: &BtwTab) -> String {
        tab.turn
            .iter()
            .find(|block| block.role == BlockRole::User)
            .and_then(|block| block.turn_model.as_ref())
            .map(|turn_model| turn_model.id.clone())
            .unwrap_or_else(|| self.props.model.clone())
    }

    fn optimistic_for_active(&self) -> Option<&BtwMessage> {
        let active = self.active_tab_id()?;
        self.optimistic
            .as_ref()
            .filter(|(thread_id, _)| thread_id == &active)
            .map(|(_, message)| message)
    }

    /// The active tab's messages, with a question still in flight.
    pub fn messages(&self) -> Vec<BtwMessage> {
        let mut messages = self
            .persisted()
            .map(|thread| thread.messages)
            .unwrap_or_default();
        if let Some(pending) = self.optimistic_for_active()
            && !messages.iter().any(|message| message.id == pending.id)
        {
            messages.push(pending.clone());
        }
        messages
    }

    /// The active tab's live reply blocks.
    pub fn pending_blocks(&self) -> Vec<Block> {
        self.persisted()
            .and_then(|thread| thread.pending_blocks)
            .unwrap_or_default()
    }

    /// The active tab is answering.
    pub fn running(&self) -> bool {
        self.persisted()
            .is_some_and(|thread| thread.status == BtwThreadStatus::Running)
            || self.optimistic_for_active().is_some()
    }

    /// The side composer's model.
    pub fn model(&self) -> String {
        if let Some(model) = &self.draft_model {
            return model.clone();
        }
        if let Some(model) = self.persisted().and_then(|thread| thread.model) {
            return model;
        }
        match self.active() {
            Some(tab) => self.base_model_for(&tab),
            None => self.props.model.clone(),
        }
    }

    /// The side composer's model settings.
    pub fn model_settings(&self, host: &dyn BtwHost) -> ModelSettings {
        if let Some(settings) = &self.draft_model_settings {
            return settings.clone();
        }
        if let Some(settings) = self.persisted().and_then(|thread| thread.model_settings) {
            return settings;
        }
        host.preferred_model_settings(
            self.harness(host),
            &self.model(),
            &self.props.model_settings,
        )
    }

    fn open_target(&self, host: &dyn BtwHost) -> Option<String> {
        host.open_target_turn_id(
            &self.turns,
            &self.props.blocks,
            self.props.harness,
            self.props.managed,
        )
    }

    /// A new tab can start: some finished turn can take a side question.
    pub fn can_start_draft(&self, host: &dyn BtwHost) -> bool {
        self.open_target(host).is_some()
    }

    /// `finishClose`: the close animation ended. Availability can disappear
    /// without an explicit close, so no latent open request survives.
    pub fn finish_close(&mut self) {
        self.requested_open = false;
        self.rendered = false;
        self.drafts.clear();
        self.draft_texts.clear();
        self.optimistic = None;
        self.draft_model = None;
        self.draft_model_settings = None;
    }

    fn send(
        &mut self,
        tab: &BtwTab,
        text: &str,
        model: &str,
        settings: &ModelSettings,
        host: &dyn BtwHost,
        cx: &mut App,
    ) -> bool {
        let message_id = new_id();
        let accepted = host.submit(
            BtwRequest {
                turn: &tab.turn,
                thread_id: &tab.id,
                message_id: &message_id,
                text,
                model: (!model.is_empty()).then_some(model),
                model_settings: settings,
            },
            cx,
        );
        if !accepted {
            return false;
        }
        self.optimistic = Some((
            tab.id.clone(),
            BtwMessage {
                id: message_id,
                role: BtwMessageRole::User,
                text: text.to_string(),
                created_at: now_ms(),
                blocks: None,
                extra: Extra::new(),
            },
        ));
        true
    }

    /// A new tab reads from the latest finished turn at the moment it opens.
    fn start_draft_tab(&mut self, host: &dyn BtwHost) -> Option<BtwTab> {
        let turn_id = self.open_target(host)?;
        let turn = self
            .turns
            .iter()
            .find(|turn| turn.first().is_some_and(|block| block.id == turn_id))?
            .clone();
        let id = new_id();
        self.drafts.push(Draft {
            id: id.clone(),
            turn_id,
        });
        self.active_id = Some(id.clone());
        self.draft_model = None;
        self.draft_model_settings = None;
        self.draft_texts.insert(id.clone(), String::new());
        self.seed = Seed {
            key: self.seed.key + 1,
            text: String::new(),
        };
        Some(BtwTab {
            id,
            turn,
            thread: None,
            question: None,
            status: None,
        })
    }

    pub fn select_tab(&mut self, id: &str) {
        if self.active_tab_id().as_deref() == Some(id) {
            return;
        }
        self.draft_model = None;
        self.draft_model_settings = None;
        self.active_id = Some(id.to_string());
        self.seed = Seed {
            key: self.seed.key + 1,
            text: self.draft_texts.get(id).cloned().unwrap_or_default(),
        };
    }

    /// `openWith`: open a new tab. `text` is sent right away, or left unsent
    /// in the side composer with `draft`.
    pub fn open_with(&mut self, text: &str, draft: bool, host: &dyn BtwHost, cx: &mut App) -> bool {
        if !self.props.available {
            return false;
        }
        let question = monocode_core::js::trim(text).to_string();
        if question.is_empty() && self.open_target(host).is_none() {
            let Some(existing) = self.active() else {
                return false;
            };
            self.active_id = Some(existing.id.clone());
            self.seed = Seed {
                key: self.seed.key + 1,
                text: self
                    .draft_texts
                    .get(&existing.id)
                    .cloned()
                    .unwrap_or_default(),
            };
            self.requested_open = true;
            self.sync_rendered();
            return true;
        }
        let previous_active_id = self.active_tab_id();
        let Some(tab) = self.start_draft_tab(host) else {
            return false;
        };
        if draft {
            self.requested_open = true;
            self.draft_texts.insert(tab.id.clone(), text.to_string());
            self.seed = Seed {
                key: self.seed.key + 1,
                text: text.to_string(),
            };
            self.sync_rendered();
            return true;
        }
        let tab_model = self.base_model_for(&tab);
        if !question.is_empty()
            && let Some(tab_harness) = self.tab_harness_for(&tab, host)
        {
            let settings =
                host.preferred_model_settings(tab_harness, &tab_model, &self.props.model_settings);
            if !self.send(&tab, &question, &tab_model, &settings, host, cx) {
                self.drafts.retain(|entry| entry.id != tab.id);
                self.draft_texts.remove(&tab.id);
                self.active_id = previous_active_id;
                return false;
            }
        }
        self.requested_open = true;
        self.sync_rendered();
        true
    }

    /// Another tab from the latest finished turn.
    pub fn start_draft(&mut self, host: &dyn BtwHost) {
        self.start_draft_tab(host);
    }

    /// `onDraftChange`: the active tab's unsent text.
    pub fn change_draft(&mut self, text: &str) {
        if let Some(id) = self.active_tab_id() {
            self.draft_texts.insert(id, text.to_string());
        }
    }

    pub fn close(&mut self) {
        self.requested_open = false;
    }

    /// Asks the active tab. False keeps the text in the composer.
    pub fn submit(&mut self, text: &str, host: &dyn BtwHost, cx: &mut App) -> bool {
        let question = monocode_core::js::trim(text).to_string();
        if question.is_empty() || self.running() {
            return false;
        }
        let Some(active) = self.active() else {
            return false;
        };
        if self.tab_harness_for(&active, host).is_none() {
            return false;
        }
        let model = self.model();
        let settings = self.model_settings(host);
        self.send(&active, &question, &model, &settings, host, cx)
    }

    /// Discards a draft tab or deletes a saved thread. Closing the last tab
    /// closes the sheet.
    pub fn close_tab(&mut self, tab_id: &str, host: &dyn BtwHost, cx: &mut App) {
        let tabs = self.tabs();
        let Some(tab) = tabs.iter().find(|tab| tab.id == tab_id).cloned() else {
            return;
        };
        let remaining: Vec<&BtwTab> = tabs.iter().filter(|entry| entry.id != tab.id).collect();
        let was_active = self.active_tab_id().as_deref() == Some(tab_id);
        self.drafts.retain(|entry| entry.id != tab.id);
        if tab.thread.is_some() {
            host.delete(&tab.turn, &tab.id, cx);
        }
        self.draft_texts.remove(&tab.id);
        if remaining.is_empty() {
            self.requested_open = false;
            return;
        }
        if was_active {
            let index = tabs
                .iter()
                .position(|entry| entry.id == tab.id)
                .unwrap_or(0);
            let next = remaining[index.min(remaining.len() - 1)].id.clone();
            // `selectTab` compares against the tab still active in this render.
            self.draft_model = None;
            self.draft_model_settings = None;
            self.active_id = Some(next.clone());
            self.seed = Seed {
                key: self.seed.key + 1,
                text: self.draft_texts.get(&next).cloned().unwrap_or_default(),
            };
        }
    }

    /// Stops the active tab's streaming answer, keeping what arrived.
    pub fn stop(&mut self, host: &dyn BtwHost, cx: &mut App) {
        let Some(active) = self.active() else {
            return;
        };
        if self
            .optimistic
            .as_ref()
            .is_some_and(|(thread_id, _)| thread_id == &active.id)
        {
            self.optimistic = None;
        }
        if active
            .thread
            .as_ref()
            .is_some_and(|thread| thread.status == BtwThreadStatus::Running)
        {
            host.stop(&active.turn, &active.id, cx);
        }
    }

    pub fn retry(&mut self, host: &dyn BtwHost, cx: &mut App) {
        if let Some(active) = self.active()
            && let Some(thread) = &active.thread
        {
            host.retry(&active.turn, &thread.id, cx);
        }
    }

    /// The side composer's model picker chose a model of its harness.
    pub fn change_model(
        &mut self,
        harness: HarnessId,
        model: &str,
        host: &dyn BtwHost,
        cx: &mut App,
    ) {
        if harness != self.harness(host) {
            return;
        }
        let settings = host.preferred_model_settings(harness, model, &self.model_settings(host));
        self.draft_model = Some(model.to_string());
        self.draft_model_settings = Some(settings.clone());
        if let Some(active) = self.active()
            && active.thread.is_some()
        {
            host.set_model(&active.turn, &active.id, model, &settings, cx);
        }
    }

    pub fn change_model_settings(
        &mut self,
        settings: ModelSettings,
        host: &dyn BtwHost,
        cx: &mut App,
    ) {
        let model = self.model();
        self.draft_model_settings = Some(settings.clone());
        if let Some(active) = self.active()
            && active.thread.is_some()
        {
            host.set_model(&active.turn, &active.id, &model, &settings, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compacts_a_question_into_a_tab_label() {
        assert_eq!(compact_question(None), "New question");
        assert_eq!(compact_question(Some("  ")), "New question");
        assert_eq!(compact_question(Some("why\n  is\tthis")), "why is this");
        let long = "a".repeat(60);
        assert_eq!(
            compact_question(Some(&long)),
            format!("{}…", "a".repeat(45))
        );
    }
}
