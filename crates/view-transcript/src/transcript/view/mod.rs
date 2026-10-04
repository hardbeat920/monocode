//! The GPUI transcript. Port of the component tree in
//! src/features/sessions/ui/AgentTranscript.tsx and the changes card in
//! SessionReview.tsx.
//!
//! [`TranscriptView`] takes a [`Session`] snapshot and lays its turns out
//! with [`build_plan`]. Each plan row is one item of a GPUI `list`, so only
//! the rows on screen are laid out, and the list follows the tail while the
//! agent streams. User actions come out as [`TranscriptEvent`]s.

mod activity;
mod blocks;
mod changes;
mod fold;
mod footer;
pub(crate) mod mascot;
mod parts;
mod shimmer;
pub mod style;
mod subagents;
mod tools;
mod user;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, ElementId, Entity, EventEmitter,
    FollowMode, InteractiveElement as _, IntoElement, ListAlignment, ListScrollEvent, ListState,
    ParentElement as _, Pixels, Render, ScrollHandle, SharedString, Styled as _, Task, WeakEntity,
    Window, canvas, div, list, px,
};
use monocode_core::appearance::TranscriptLayout;
use monocode_core::block::ModelTarget;
use monocode_core::models::ModelCatalog;
use monocode_core::transcript::BlockRef;
use monocode_core::transcript::paths::resolve_workspace_path;
use monocode_core::{BlockRole, Session};
use monocode_markdown::{MarkdownStyle, MarkdownView};
use monocode_ui::{Theme, u};

pub use monocode_core::harness_event::ApprovalDecision;

use crate::cards::TranscriptCardEvent;
use crate::cards::generated_image::GeneratedImage;
use crate::cards::link_preview::UserLinkPreview;
use crate::cards::selection_menu::{
    SelectionMenuEvent, TranscriptSelection, TranscriptSelectionMenu,
};
use crate::threads::{
    CatalogMenuSource, ModelMenuSource, NoRuns, OrchestrationActions, OrchestrationPreview,
    OrchestrationRuns, OrchestratorConstellation, SecondOpinionButton,
};
use crate::transcript::model::plan::{
    BlockStore, FoldTitle, PlanCache, PlanOptions, PlanState, Row, RowKind, build_plan,
    visible_blocks,
};
use crate::transcript::model::turn::ElapsedClock;
use style::{BOTTOM_PADDING, COLUMN_MAX_WIDTH, MarkdownVariant, markdown_style};

/// Bind the markdown keys the transcript's messages use. Call once at startup.
pub fn init(cx: &mut App) {
    monocode_markdown::init(cx);
}

/// `CheckpointFile`: one file the session changed, for the changes card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub relative: String,
    pub status: String,
    pub additions: i64,
    pub deletions: i64,
    /// False when changes between this session's edits prevent an exact diff.
    pub exact: bool,
    /// False when restoring could overwrite a change made outside this session.
    pub undoable: bool,
}

/// What the host offers and how the transcript is shown: the
/// `AgentTranscript` props that are not the session itself.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptConfig {
    pub layout: TranscriptLayout,
    /// `monocode.transcriptAnchor`: a sent prompt sits at the top.
    pub anchor_prompts: bool,
    /// A worker's transcript: show the orchestrator's turns.
    pub managed: bool,
    /// False while another tab is in front.
    pub visible: bool,
    /// Kept alive after its pane closed (see `TranscriptPool`).
    pub parked: bool,
    /// For model names on live turns and subagent rows.
    pub catalog: Arc<ModelCatalog>,
    /// The host handles approvals (`onApproval`).
    pub approvals: bool,
    pub can_edit_last_turn: bool,
    pub editing_last_turn: bool,
    pub can_save_notes: bool,
    pub can_second_opinion: bool,
    pub can_handoff: bool,
    pub can_open_plans: bool,
    pub can_build_plans: bool,
    pub can_build_plan_targets: bool,
    pub can_send_drafts: bool,
    /// The host takes selected text into the composer
    /// ([`TranscriptCardEvent::AddToChat`]).
    pub can_add_to_chat: bool,
}

impl Default for TranscriptConfig {
    fn default() -> Self {
        Self {
            layout: TranscriptLayout::Chat,
            anchor_prompts: true,
            managed: false,
            visible: true,
            parked: false,
            catalog: Arc::new(ModelCatalog::new()),
            approvals: true,
            can_edit_last_turn: false,
            editing_last_turn: false,
            can_save_notes: false,
            can_second_opinion: false,
            can_handoff: false,
            can_open_plans: false,
            can_build_plans: false,
            can_build_plan_targets: false,
            can_send_drafts: false,
            can_add_to_chat: false,
        }
    }
}

/// What the reader did. The host acts on these; the transcript only draws.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptEvent {
    Approval {
        request_id: i64,
        decision: ApprovalDecision,
    },
    OpenFile {
        path: String,
        line: Option<i64>,
    },
    /// Open a file's diff (an edit row's chip).
    OpenDiff {
        path: String,
    },
    OpenUrl {
        url: String,
    },
    OpenPlan {
        block_id: String,
    },
    BuildPlan {
        block_id: String,
    },
    BuildPlanWithTarget {
        block_id: String,
        target: ModelTarget,
    },
    SendDraft {
        block_id: String,
    },
    RemoveDraft {
        block_id: String,
    },
    EditLastTurn,
    /// Text the transcript wrote to the clipboard (for the copy cue).
    Copied {
        text: String,
    },
    SaveNote {
        text: String,
    },
    SecondOpinion {
        turn_id: String,
        target: ModelTarget,
    },
    Handoff {
        turn_id: String,
        target: ModelTarget,
    },
    UndoChanges,
    KeepChanges,
    /// Open the changes review, at `path` when one file was picked.
    ReviewChanges {
        path: Option<String>,
    },
    /// The reader scrolled away from the bottom, or back.
    JumpToBottomChanged {
        show: bool,
    },
}

/// Which markdown entity a block draws through. One block can show in more
/// than one place (a headline and its expanded body).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MarkdownKey {
    pub id: String,
    pub slot: MarkdownSlot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MarkdownSlot {
    /// An assistant reply or note.
    Prose,
    /// A thought opened from its one-line summary.
    Reasoning,
    /// A subagent's report.
    Report,
}

struct MarkdownEntry {
    view: Entity<MarkdownView>,
    variant: MarkdownVariant,
}

/// The transcript view entity.
pub struct TranscriptView {
    pub(crate) config: TranscriptConfig,
    session: Option<Arc<Session>>,
    store: BlockStore,
    pub(crate) blocks: Vec<BlockRef>,
    plan_cache: PlanCache,
    pub(crate) rows: Vec<Row>,
    list: ListState,
    pub(crate) state: PlanState,
    search_query: String,
    /// Disclosure overrides by key, such as `phase:<id>`.
    toggles: HashMap<String, bool>,
    /// When a copy or save button last succeeded, by key.
    feedback: HashMap<String, Instant>,
    markdown: HashMap<MarkdownKey, MarkdownEntry>,
    styles: HashMap<MarkdownVariant, MarkdownStyle>,
    clocks: HashMap<String, ElapsedClock>,
    scrolls: HashMap<String, ScrollHandle>,
    pub(crate) changes: Vec<ChangedFile>,
    pub(crate) undo_locked: bool,
    pub(crate) changes_busy: bool,
    anchor_turn: bool,
    last_user_id: Option<String>,
    /// Whether `last_user_id` has been read for this session yet.
    user_seen: bool,
    show_jump: bool,
    ticker: Option<Task<()>>,
    feedback_timer: Option<Task<()>>,
    theme_epoch: u64,
    /// The list's width in the last frame, for measuring text.
    width: Rc<Cell<Pixels>>,
    /// Link chips in prompts, by block id.
    pub(crate) link_cards: HashMap<String, Entity<UserLinkPreview>>,
    /// Generated images, by block id.
    pub(crate) image_cards: HashMap<String, Entity<GeneratedImage>>,
    pub(crate) attachment_cards: HashMap<(String, String), Entity<GeneratedImage>>,
    pub(crate) attachment_seen: HashMap<String, BlockRef>,
    /// Orchestrator turn bursts, by prompt block id.
    pub(crate) constellations: HashMap<String, Entity<OrchestratorConstellation>>,
    /// Orchestration assignment cards, by proposal block id.
    pub(crate) orchestration_cards: HashMap<String, Entity<OrchestrationPreview>>,
    /// What each card was last given: the block, busy, and the catalog.
    pub(crate) orchestration_seen: HashMap<String, (BlockRef, bool, Arc<ModelCatalog>)>,
    /// The orchestrator the assignment cards read runs from and act through.
    orchestration_runs: Option<Rc<dyn OrchestrationRuns>>,
    orchestration_actions: Option<Rc<dyn OrchestrationActions>>,
    /// "Add to chat" and "Add to notes" over selected text.
    selection_menu: Entity<TranscriptSelectionMenu>,
    model_menu_source: Option<Rc<dyn ModelMenuSource>>,
    turn_model_menus: HashMap<String, (Entity<SecondOpinionButton>, gpui::Subscription)>,
    _theme: gpui::Subscription,
    _selection: gpui::Subscription,
}

impl EventEmitter<TranscriptEvent> for TranscriptView {}
impl EventEmitter<TranscriptCardEvent> for TranscriptView {}

/// Epoch ms now, `Date.now()`.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// An element id scoped to a row.
pub(crate) fn eid(key: &str, part: &str) -> ElementId {
    ElementId::Name(SharedString::from(format!("{key}:{part}")))
}

impl TranscriptView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // One item more than there are rows: the bottom padding.
        let list = ListState::new(1, ListAlignment::Top, px(1200.));
        list.set_follow_mode(FollowMode::Tail);
        let weak = cx.entity().downgrade();
        list.set_scroll_handler(move |event: &ListScrollEvent, _, cx| {
            // The list re-engages the tail on its next layout, after this
            // event, so reaching the bottom padding counts as back.
            let at_bottom = event.is_following_tail || event.visible_range.end >= event.count;
            weak.update(cx, |this, cx| {
                this.set_show_jump(!at_bottom, cx);
                // Scrolling moves the text out from under the selection menu.
                this.dismiss_selection_menu(cx);
            })
            .ok();
        });
        let theme = cx.observe_global::<Theme>(|this, cx| this.theme_changed(cx));
        let selection_menu = cx.new(|_| TranscriptSelectionMenu::new(false, false));
        let selection = cx.subscribe(
            &selection_menu,
            |this, menu, event: &SelectionMenuEvent, cx| this.selection_menu_event(menu, event, cx),
        );
        Self {
            config: TranscriptConfig::default(),
            session: None,
            store: BlockStore::default(),
            blocks: Vec::new(),
            plan_cache: PlanCache::default(),
            rows: Vec::new(),
            list,
            state: PlanState::default(),
            search_query: String::new(),
            toggles: HashMap::new(),
            feedback: HashMap::new(),
            markdown: HashMap::new(),
            styles: HashMap::new(),
            clocks: HashMap::new(),
            scrolls: HashMap::new(),
            changes: Vec::new(),
            undo_locked: false,
            changes_busy: false,
            anchor_turn: false,
            last_user_id: None,
            user_seen: false,
            show_jump: false,
            ticker: None,
            feedback_timer: None,
            theme_epoch: 0,
            width: Rc::new(Cell::new(px(0.))),
            link_cards: HashMap::new(),
            image_cards: HashMap::new(),
            attachment_cards: HashMap::new(),
            attachment_seen: HashMap::new(),
            constellations: HashMap::new(),
            orchestration_cards: HashMap::new(),
            orchestration_seen: HashMap::new(),
            orchestration_runs: None,
            orchestration_actions: None,
            selection_menu,
            model_menu_source: None,
            turn_model_menus: HashMap::new(),
            _theme: theme,
            _selection: selection,
        }
    }

    /// The session snapshot currently shown.
    pub fn session(&self) -> Option<&Arc<Session>> {
        self.session.as_ref()
    }

    /// Show a session snapshot. Unchanged blocks keep their laid out rows.
    pub fn set_session(&mut self, session: Arc<Session>, cx: &mut Context<Self>) {
        let switched = self
            .session
            .as_ref()
            .is_none_or(|current| current.id != session.id);
        if switched {
            self.store = BlockStore::default();
            self.plan_cache = PlanCache::default();
            self.toggles.clear();
            self.state = PlanState::default();
            self.clocks.clear();
            self.markdown.clear();
            self.link_cards.clear();
            self.image_cards.clear();
            self.attachment_cards.clear();
            self.attachment_seen.clear();
            self.constellations.clear();
            self.orchestration_cards.clear();
            self.orchestration_seen.clear();
            self.turn_model_menus.clear();
            self.anchor_turn = session.is_busy();
            self.last_user_id = None;
            self.user_seen = false;
            self.list.reset(1);
            self.rows.clear();
        }
        let changed = self.store.update(&session.blocks);
        let busy_changed = self.session.as_ref().is_none_or(|current| {
            current.busy != session.busy
                || current.background_tasks != session.background_tasks
                || current.pending_question.is_some() != session.pending_question.is_some()
                || current.model != session.model
                || current.harness != session.harness
        });
        self.session = Some(session);
        if switched || changed || busy_changed {
            self.rebuild(cx);
        }
    }

    pub fn config(&self) -> &TranscriptConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: TranscriptConfig, cx: &mut Context<Self>) {
        if self.config == config {
            return;
        }
        let was_parked = self.config.parked;
        let busy = self
            .session
            .as_ref()
            .is_some_and(|session| session.is_busy());
        if was_parked && !config.parked && self.anchor_turn != busy {
            self.anchor_turn = busy;
        }
        if !was_parked && config.parked {
            self.state.search_current = None;
            self.search_query.clear();
        }
        let (chat, notes) = (config.can_add_to_chat, config.can_save_notes);
        self.selection_menu
            .update(cx, |menu, cx| menu.set_actions(chat, notes, cx));
        self.config = config;
        if self.model_menu_source.is_none() {
            let source = self.model_menu_source();
            for (view, _) in self.turn_model_menus.values() {
                view.update(cx, |view, cx| view.set_source(source.clone(), cx));
            }
        }
        self.rebuild(cx);
    }

    /// The files the session changed, for the card after the latest reply.
    pub fn set_changes(
        &mut self,
        files: Vec<ChangedFile>,
        undo_locked: bool,
        cx: &mut Context<Self>,
    ) {
        if self.changes == files && self.undo_locked == undo_locked {
            return;
        }
        if files.len() <= 3 {
            self.toggles.remove("review:expanded");
        }
        self.changes = files;
        self.undo_locked = undo_locked;
        self.changes_busy = false;
        self.rebuild(cx);
    }

    /// Scroll to the newest output and follow it again.
    pub fn jump_to_bottom(&mut self, cx: &mut Context<Self>) {
        self.list.set_follow_mode(FollowMode::Tail);
        self.set_show_jump(false, cx);
        cx.notify();
    }

    /// Scroll to the first turn and stop following the tail.
    pub fn scroll_to_top(&mut self, cx: &mut Context<Self>) {
        self.list.scroll_to(gpui::ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        self.set_show_jump(true, cx);
        cx.notify();
    }

    /// Scroll so turn `index` (in reading order) starts at the top.
    pub fn scroll_to_turn(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.iter().position(|row| row.turn_index >= index) {
            self.list.scroll_to(gpui::ListOffset {
                item_ix: row,
                offset_in_item: px(0.),
            });
            self.set_show_jump(true, cx);
            cx.notify();
        }
    }

    /// Open every turn's folded work.
    pub fn open_all_work(&mut self, cx: &mut Context<Self>) {
        let turns: Vec<String> = self
            .rows
            .iter()
            .filter(|row| matches!(&row.kind, RowKind::FoldLine(line) if line.expandable))
            .map(|row| row.turn_id.clone())
            .collect();
        for turn in turns {
            self.state.open_work.insert(turn, true);
        }
        self.rebuild(cx);
    }

    /// Whether the reader has scrolled away from the bottom.
    pub fn is_scrolled_away(&self) -> bool {
        self.show_jump
    }

    /// `revealBlock`: whether a turn holds `block_id`. Every turn has rows,
    /// so there is nothing to load.
    pub fn reveal_block(&self, block_id: &str) -> bool {
        self.blocks.iter().any(|block| block.id == block_id)
    }

    /// `navigateToBlock`: open the fold that holds `block_id`, mark it as the
    /// current search result, and scroll it into view. `None` clears the
    /// search.
    pub fn navigate_to_block(
        &mut self,
        block_id: Option<&str>,
        query: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(block_id) = block_id else {
            self.state.search_current = None;
            self.search_query.clear();
            self.rebuild(cx);
            return true;
        };
        let turns =
            monocode_core::transcript::activity::group_turns(&self.blocks, self.config.managed);
        let Some(turn) = turns
            .iter()
            .find(|turn| turn.iter().any(|block| block.id == block_id))
        else {
            return false;
        };
        self.state.open_work.insert(turn[0].id.clone(), true);
        self.state.search_current = Some(block_id.to_string());
        self.search_query = query.to_string();
        self.rebuild(cx);
        let row = self
            .rows
            .iter()
            .position(|row| row.search_current)
            .or_else(|| self.rows.iter().position(|row| row.turn_id == turn[0].id));
        if let Some(row) = row {
            self.list.scroll_to_reveal_item(row);
            self.set_show_jump(true, cx);
        }
        cx.notify();
        true
    }

    /// The current search, for highlighting matches in drawn text.
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Where row `ix` was drawn in the last frame, if it was on screen.
    pub fn row_bounds(&self, ix: usize) -> Option<gpui::Bounds<Pixels>> {
        self.list.bounds_for_item(ix)
    }

    /// How many markdown views exist. Rows build them as they come on
    /// screen, so a long session has far fewer than it has replies.
    pub fn markdown_view_count(&self) -> usize {
        self.markdown.len()
    }

    /// The markdown view a block's reply draws through, if it was drawn.
    pub fn markdown_for(&self, block_id: &str) -> Option<Entity<MarkdownView>> {
        self.markdown
            .get(&MarkdownKey {
                id: block_id.to_string(),
                slot: MarkdownSlot::Prose,
            })
            .map(|entry| entry.view.clone())
    }

    /// Plan rows, for tests and tools that inspect the layout.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    fn set_show_jump(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_jump != show {
            self.show_jump = show;
            cx.emit(TranscriptEvent::JumpToBottomChanged { show });
        }
    }

    fn theme_changed(&mut self, cx: &mut Context<Self>) {
        self.styles.clear();
        self.theme_epoch += 1;
        let entries: Vec<(Entity<MarkdownView>, MarkdownVariant)> = self
            .markdown
            .values()
            .map(|entry| (entry.view.clone(), entry.variant))
            .collect();
        for (view, variant) in entries {
            let style = self.style(variant, cx);
            view.update(cx, |view, cx| view.set_style(style, cx));
        }
        self.list.remeasure();
        cx.notify();
    }

    fn style(&mut self, variant: MarkdownVariant, cx: &App) -> MarkdownStyle {
        self.styles
            .entry(variant)
            .or_insert_with(|| markdown_style(Theme::of(cx), variant))
            .clone()
    }

    fn plan_options(&self) -> PlanOptions {
        let session = self.session.as_deref();
        let harness = session.map(|session| session.harness);
        let current_model_name = session.map(|session| {
            self.config
                .catalog
                .resolve_model(session.harness, Some(&session.model))
                .name
        });
        PlanOptions {
            busy: session.is_some_and(Session::is_busy),
            managed: self.config.managed,
            visible: self.config.visible,
            harness,
            current_model_name,
            pending_question: session.is_some_and(|session| session.pending_question.is_some()),
            background_tasks: session
                .and_then(|session| session.background_tasks.clone())
                .unwrap_or_default(),
            has_accessory: !self.config.parked
                && !self.changes.is_empty()
                && !session.is_some_and(Session::is_busy),
            can_edit_last_turn: self.config.can_edit_last_turn,
            editing_last_turn: self.config.editing_last_turn,
        }
    }

    /// Lay the turns out again and tell the list which rows changed.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let options = self.plan_options();
        self.blocks = visible_blocks(self.store.blocks(), options.harness);
        let rows = build_plan(
            &self.blocks,
            &options,
            &self.state,
            Some(&mut self.plan_cache),
        );
        self.apply_rows(rows);
        self.sync_anchor(&options);
        self.prune_markdown();
        self.sync_ticker(cx);
        cx.notify();
    }

    /// Splice only the rows that changed. A row that kept its key at the
    /// same place is remeasured instead, which keeps its height estimate.
    fn apply_rows(&mut self, rows: Vec<Row>) {
        let old = std::mem::take(&mut self.rows);
        let mut prefix = 0;
        while prefix < old.len() && prefix < rows.len() && old[prefix].same_as(&rows[prefix]) {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < old.len() - prefix
            && suffix < rows.len() - prefix
            && old[old.len() - 1 - suffix].same_as(&rows[rows.len() - 1 - suffix])
        {
            suffix += 1;
        }
        let old_mid = &old[prefix..old.len() - suffix];
        let new_mid = &rows[prefix..rows.len() - suffix];
        let same_keys = old_mid.len() == new_mid.len()
            && old_mid
                .iter()
                .zip(new_mid)
                .all(|(old, new)| old.key == new.key);
        if same_keys {
            if !new_mid.is_empty() {
                self.list.remeasure_items(prefix..prefix + new_mid.len());
            }
        } else {
            self.list.splice(prefix..old.len() - suffix, new_mid.len());
        }
        self.rows = rows;
    }

    /// A sent prompt rises to the top: the last turn reserves a viewport
    /// (`.transcript-turn-anchor`), and each new prompt pins the bottom.
    fn sync_anchor(&mut self, options: &PlanOptions) {
        let last_user = self.blocks.iter().rposition(|block| {
            block.role == BlockRole::User && (options.managed || !block.is_internal())
        });
        let last_user_id = last_user.map(|index| self.blocks[index].id.clone());
        if !self.user_seen {
            self.user_seen = true;
            self.last_user_id = last_user_id;
        } else if last_user_id != self.last_user_id {
            self.last_user_id = last_user_id.clone();
            if last_user_id.is_some() {
                self.anchor_turn = true;
            }
            self.list.set_follow_mode(FollowMode::Tail);
        }
        let last_turn_start = self.rows.iter().position(|row| row.last_turn);
        // The last turn has a user block when it starts at or before the last prompt.
        let last_turn_has_prompt = match (last_turn_start, last_user) {
            (Some(start), Some(user)) => {
                let turn_id = &self.rows[start].turn_id;
                self.blocks
                    .iter()
                    .position(|block| &block.id == turn_id)
                    .is_some_and(|turn_start| turn_start <= user)
            }
            _ => false,
        };
        let anchored = self.config.anchor_prompts && self.anchor_turn && last_turn_has_prompt;
        self.list.set_tail_reservation(
            last_turn_start
                .filter(|_| anchored)
                .map(|start| (start, px(0.))),
        );
    }

    /// Drop markdown views of blocks that left the transcript.
    fn prune_markdown(&mut self) {
        if self.markdown.len() < 64 && self.attachment_cards.is_empty() {
            return;
        }
        let mut live: HashSet<&str> = HashSet::new();
        for block in &self.blocks {
            live.insert(&block.id);
            if let Some(run) = &block.agent_run {
                for step in &run.steps {
                    live.insert(&step.id);
                }
            }
        }
        self.markdown
            .retain(|key, _| live.contains(key.id.as_str()));
        self.link_cards.retain(|id, _| live.contains(id.as_str()));
        self.image_cards.retain(|id, _| live.contains(id.as_str()));
        self.attachment_cards
            .retain(|(block, _), _| live.contains(block.as_str()));
        self.attachment_seen
            .retain(|id, _| live.contains(id.as_str()));
        self.constellations
            .retain(|id, _| live.contains(id.as_str()));
        self.orchestration_cards
            .retain(|id, _| live.contains(id.as_str()));
        self.orchestration_seen
            .retain(|id, _| live.contains(id.as_str()));
        self.turn_model_menus.retain(|id, _| {
            id.split_once(':')
                .is_some_and(|(_, block)| live.contains(block))
        });
    }

    /// The orchestrator behind the assignment cards
    /// (`OrchestrationActions` and `orchestrator` in React). Without it the
    /// cards are read-only and show no run.
    pub fn set_orchestration(
        &mut self,
        runs: Rc<dyn OrchestrationRuns>,
        actions: Option<Rc<dyn OrchestrationActions>>,
        cx: &mut Context<Self>,
    ) {
        self.orchestration_runs = Some(runs);
        self.orchestration_actions = actions;
        // The runs source is fixed per card, so the cards start over.
        self.orchestration_cards.clear();
        self.orchestration_seen.clear();
        self.list.remeasure();
        cx.notify();
    }

    /// The footer's provider, model, and effort menus read the app's live
    /// catalog, saved preferences, and installed providers.
    pub fn set_model_menu_source(
        &mut self,
        source: Rc<dyn ModelMenuSource>,
        cx: &mut Context<Self>,
    ) {
        self.model_menu_source = Some(source.clone());
        for (view, _) in self.turn_model_menus.values() {
            view.update(cx, |view, cx| view.set_source(source.clone(), cx));
        }
        cx.notify();
    }

    fn model_menu_source(&self) -> Rc<dyn ModelMenuSource> {
        self.model_menu_source.clone().unwrap_or_else(|| {
            Rc::new(CatalogMenuSource::new(
                self.config.catalog.as_ref().clone(),
                Default::default(),
                Default::default(),
            ))
        })
    }

    pub(crate) fn orchestration_providers(
        &self,
    ) -> (
        Rc<dyn OrchestrationRuns>,
        Option<Rc<dyn OrchestrationActions>>,
    ) {
        (
            self.orchestration_runs
                .clone()
                .unwrap_or_else(|| Rc::new(NoRuns)),
            self.orchestration_actions.clone(),
        )
    }

    /// After a drag ends over a reply, offer the selected text to the chat
    /// and to notes (`TranscriptSelectionMenu`). Only text inside one settled
    /// reply counts.
    fn offer_selection(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        if !self.config.can_add_to_chat && !self.config.can_save_notes {
            return;
        }
        let mut found: Option<(String, bool)> = None;
        for (key, entry) in &self.markdown {
            let view = entry.view.read(cx);
            let Some(text) = view.selected_text() else {
                continue;
            };
            let under_pointer = view
                .rendered_text()
                .iter()
                .any(|rendered| rendered.bounds.contains(&position));
            let candidate = monocode_core::transcript::selection::TranscriptSelectionCandidate {
                text: &text,
                collapsed: false,
                anchor_response_id: (!view.is_streaming()).then_some(key.id.as_str()),
                focus_response_id: (!view.is_streaming()).then_some(key.id.as_str()),
            };
            let Some(text) =
                monocode_core::transcript::selection::validate_transcript_selection(&candidate)
            else {
                continue;
            };
            if found.as_ref().is_none_or(|(_, hit)| !hit && under_pointer) {
                found = Some((text, under_pointer));
            }
        }
        let selection = found.map(|(text, _)| TranscriptSelection {
            text,
            rect: gpui::Bounds::new(position, gpui::size(px(1.), px(1.))),
        });
        self.selection_menu
            .update(cx, |menu, cx| menu.set_selection(selection, cx));
    }

    /// The "Add to chat" and "Add to notes" menu over selected text.
    pub fn selection_menu(&self) -> &Entity<TranscriptSelectionMenu> {
        &self.selection_menu
    }

    fn dismiss_selection_menu(&mut self, cx: &mut Context<Self>) {
        if self.selection_menu.read(cx).selection().is_some() {
            self.selection_menu
                .update(cx, |menu, cx| menu.dismiss(false, cx));
        }
    }

    fn selection_menu_event(
        &mut self,
        menu: Entity<TranscriptSelectionMenu>,
        event: &SelectionMenuEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SelectionMenuEvent::AddToChat { text } => {
                cx.emit(TranscriptCardEvent::AddToChat { text: text.clone() })
            }
            SelectionMenuEvent::AddToNotes { text } => {
                // `SaveNote` reports no result, so the save counts as done.
                cx.emit(TranscriptEvent::SaveNote { text: text.clone() });
                menu.update(cx, |menu, cx| menu.finish_note(Ok(()), cx));
            }
            SelectionMenuEvent::Dismiss { clear_selection } => {
                if *clear_selection {
                    let views: Vec<Entity<MarkdownView>> = self
                        .markdown
                        .values()
                        .map(|entry| entry.view.clone())
                        .collect();
                    for view in views {
                        view.update(cx, |view, cx| view.clear_selection(cx));
                    }
                }
            }
        }
    }

    /// Tick once a second while a turn's clock is on screen.
    fn sync_ticker(&mut self, cx: &mut Context<Self>) {
        let live = self.config.visible
            && self
                .rows
                .iter()
                .any(|row| matches!(&row.kind, RowKind::FoldLine(line) if matches!(line.title, FoldTitle::Live { .. })));
        if !live {
            self.ticker = None;
            return;
        }
        if self.ticker.is_some() {
            return;
        }
        self.ticker = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
    }

    pub(crate) fn toggled(&self, key: &str, default: bool) -> bool {
        self.toggles.get(key).copied().unwrap_or(default)
    }

    /// Flip a disclosure, starting from `default` the first time.
    pub(crate) fn toggle(&mut self, key: String, default: bool, cx: &mut Context<Self>) {
        let next = !self.toggled(&key, default);
        self.toggles.insert(key, next);
        cx.notify();
    }

    pub(crate) fn toggle_work(&mut self, turn_id: &str, cx: &mut Context<Self>) {
        let open = self.state.open_work.get(turn_id).copied().unwrap_or(false);
        self.state.open_work.insert(turn_id.to_string(), !open);
        self.rebuild(cx);
    }

    /// The elapsed clock of a live turn.
    pub(crate) fn elapsed(&mut self, turn_id: &str, started_at: Option<i64>, paused: bool) -> i64 {
        self.clocks
            .entry(turn_id.to_string())
            .or_default()
            .elapsed(started_at, paused, now_ms())
    }

    pub(crate) fn scroll_handle(&mut self, key: &str) -> ScrollHandle {
        self.scrolls.entry(key.to_string()).or_default().clone()
    }

    /// Copy `text`, show the check on `key` for two seconds.
    pub(crate) fn copy(&mut self, key: String, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        cx.emit(TranscriptEvent::Copied { text });
        self.flash(key, cx);
    }

    pub(crate) fn flash(&mut self, key: String, cx: &mut Context<Self>) {
        self.feedback.insert(key, Instant::now());
        self.feedback_timer = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(2000))
                .await;
            this.update(cx, |this, cx| {
                this.feedback
                    .retain(|_, at| at.elapsed() < Duration::from_millis(1900));
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub(crate) fn flashed(&self, key: &str) -> bool {
        self.feedback
            .get(key)
            .is_some_and(|at| at.elapsed() < Duration::from_millis(2000))
    }

    /// A link in rendered markdown or a chip: files open in the app, web
    /// links in the browser.
    pub(crate) fn open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        let cwd = self.session.as_ref().map(|session| session.cwd.clone());
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
            cx.emit(TranscriptEvent::OpenUrl {
                url: url.to_string(),
            });
            return;
        }
        if let Some(path) = resolve_workspace_path(url, cwd.as_deref()) {
            cx.emit(TranscriptEvent::OpenFile { path, line: None });
        }
    }

    pub(crate) fn cwd(&self) -> Option<String> {
        self.session.as_ref().map(|session| session.cwd.clone())
    }

    /// The markdown view for a block, created on first use and kept in sync
    /// with the block's text.
    pub(crate) fn markdown_view(
        &mut self,
        id: &str,
        slot: MarkdownSlot,
        text: &str,
        streaming: bool,
        variant: MarkdownVariant,
        cx: &mut Context<Self>,
    ) -> Entity<MarkdownView> {
        let key = MarkdownKey {
            id: id.to_string(),
            slot,
        };
        let style = self.style(variant, cx);
        if let Some(entry) = self.markdown.get_mut(&key) {
            let view = entry.view.clone();
            let restyle = entry.variant != variant;
            entry.variant = variant;
            view.update(cx, |view, cx| {
                if restyle {
                    view.set_style(style, cx);
                }
                view.set_text(text, cx);
                view.set_streaming(streaming, cx);
            });
            return view;
        }
        let weak = cx.entity().downgrade();
        let reasoning = slot == MarkdownSlot::Reasoning;
        let text = text.to_string();
        let view = cx.new(|cx| {
            let mut view = if streaming {
                let mut view = MarkdownView::new(cx);
                view.set_streaming(true, cx);
                view.set_text(&text, cx);
                view
            } else {
                MarkdownView::with_text(text, cx)
            };
            view.set_style(style, cx);
            view.set_reasoning(reasoning, cx);
            view.on_link_click(move |link, _, cx| {
                let url = link.url.to_string();
                weak.update(cx, |this, cx| this.open_link(&url, cx)).ok();
            });
            view
        });
        self.markdown.insert(
            key,
            MarkdownEntry {
                view: view.clone(),
                variant,
            },
        );
        view
    }

    /// One list item.
    fn render_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(ix).cloned() else {
            return div().h(u(BOTTOM_PADDING)).into_any_element();
        };
        let first_of_turn =
            row.turn_index > 0 && ix > 0 && self.rows[ix - 1].turn_index != row.turn_index;
        let content = match &row.kind {
            RowKind::Item {
                item,
                index,
                view,
                placement,
            } => self.render_item(&row, item, *index, *view, *placement, window, cx),
            RowKind::FoldLine(line) => self.render_fold_line(&row, line, cx),
            RowKind::Proposal(block) => self.render_proposal(&row, block, window, cx),
            RowKind::Accessory => self.render_changes(&row, cx),
            RowKind::Footer(footer) => self.render_footer(&row, footer, cx),
        };
        div()
            .id(ElementId::Name(SharedString::from(row.key.clone())))
            .w_full()
            .flex()
            .justify_center()
            .when(first_of_turn, |el| el.pt(u(4.)))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .max_w(u(COLUMN_MAX_WIDTH))
                    .child(content),
            )
            .into_any_element()
    }

    /// The width the transcript column lays text out in, for measuring.
    pub(crate) fn column_width(&self, window: &Window) -> Pixels {
        // The list state is borrowed while it lays rows out, so read the width
        // the last frame recorded.
        let viewport = self.width.get();
        let max = u(COLUMN_MAX_WIDTH).to_pixels(window.rem_size());
        if viewport <= px(0.) {
            max
        } else {
            viewport.min(max)
        }
    }
}

impl Render for TranscriptView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .id("agent-transcript")
            .size_full()
            .font_family(theme.fonts.mono.clone())
            .text_size(u(13.))
            .line_height(u(20.))
            .text_color(theme.colors.content)
            .relative()
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                    this.offer_selection(event.position, cx)
                }),
            )
            .child(
                list(
                    self.list.clone(),
                    cx.processor(|this, ix, window, cx| this.render_row(ix, window, cx)),
                )
                .size_full(),
            )
            .child({
                let width = self.width.clone();
                canvas(
                    move |bounds, _, _| width.set(bounds.size.width),
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full()
            })
            .child(self.selection_menu.clone())
    }
}
