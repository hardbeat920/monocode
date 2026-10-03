//! The turn layout `AgentTranscriptComponent` computes in
//! src/features/sessions/ui/AgentTranscript.tsx, as data.
//!
//! Each turn becomes a run of [`Row`]s: its items, the fold line the work
//! folds behind, the open fold's body, orchestration results, the session
//! accessory, and the action row. The view gives each row its own list item,
//! so a long session only lays out what is on screen.
//!
//! React paged turns in ("Load earlier messages", 20 at a time) to keep the
//! DOM small. A virtualized list makes that unnecessary, so every turn gets
//! rows from the start.

use std::collections::HashMap;
use std::sync::Arc;

use monocode_core::block::TurnMetrics;
use monocode_core::orchestration::OrchestrationProposalStatus;
use monocode_core::{Block, BlockRole, HarnessId};

use super::support::{is_harness_auth_error, last_user_turn_block, supports_harness_login};
use super::turn::{format_working_duration, turn_user_block};
use monocode_core::transcript::activity::{
    ActivityPhaseKind, BlockRef, TurnItem, activity_still_running, first_foldable_index,
    foldable_work, folded_blocks, group_turn_items, group_turns, initial_thinking_index,
    is_prose_block, last_activity_index, needs_approval, turn_copy_text, work_kind,
    work_summary_line,
};

/// What the transcript was given besides its blocks: the `AgentTranscript`
/// props that shape the layout.
#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    pub busy: bool,
    /// A worker's transcript: show the orchestrator's turns.
    pub managed: bool,
    /// False while another tab is in front.
    pub visible: bool,
    pub harness: Option<HarnessId>,
    /// `resolveModel(harness, model).name`, for live turns without a recorded model.
    pub current_model_name: Option<String>,
    pub pending_question: bool,
    /// Work the agent left running when it yielded.
    pub background_tasks: Vec<String>,
    /// A session accessory (the changes card) shows after the latest reply.
    pub has_accessory: bool,
    /// The host offers "Edit and resend" on the last turn.
    pub can_edit_last_turn: bool,
    pub editing_last_turn: bool,
}

/// UI state the layout reads.
#[derive(Debug, Clone, Default)]
pub struct PlanState {
    /// Turns whose folded work the reader opened, by turn id.
    pub open_work: HashMap<String, bool>,
    /// The search result being shown.
    pub search_current: Option<String>,
}

/// What a fold line says.
#[derive(Debug, Clone, PartialEq)]
pub enum FoldTitle {
    /// `LiveFoldTitle`: the ticking clock, or what the turn waits on.
    Live {
        started_at: Option<i64>,
        paused: bool,
        waiting_label: Option<&'static str>,
        background: Vec<String>,
        model_name: Option<String>,
    },
    Text(String),
}

/// `WorkFoldLine`.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldLine {
    pub title: FoldTitle,
    pub kind: ActivityPhaseKind,
    pub harness: Option<HarnessId>,
    pub live: bool,
    /// There is folded work to open.
    pub expandable: bool,
    pub open: bool,
}

/// How an item renders (`renderItem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemView {
    /// `SubagentStack`.
    Subagents { live: bool },
    /// `InitialThinking`: reasoning before the first response.
    InitialThinking { live: bool },
    /// `ActivityPhases`.
    Activity { done: bool },
    /// `TranscriptBlock`.
    Block {
        /// Prose with something directly above it in the turn.
        under_line: bool,
        /// Shows the "Edit and resend" button.
        can_edit: bool,
        editing: bool,
    },
}

/// Where an item sits relative to the fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// An ordinary row of the turn.
    Plain,
    /// Inside the open fold, on its rail (`zen-fold-rail`). `tail` is the
    /// last row (`zen-fold-tail`) and `prose` marks mid-run prose
    /// (`zen-fold-prose`).
    FoldBody { tail: bool, prose: bool },
    /// A subagent stack the fold spans, parked under the fold line.
    FoldSubagents,
}

/// `TurnDuration`: the action row under a finished turn.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnFooter {
    pub elapsed_ms: i64,
    pub metrics: Option<TurnMetrics>,
    /// The fold line above already keeps the time.
    pub label_hidden: bool,
    pub model_name: Option<String>,
    pub completed_at: Option<i64>,
    /// `turnCopyText`, empty when the turn has nothing to copy.
    pub copy_text: String,
    pub harness: Option<HarnessId>,
    /// The turn's own model id, so a same-harness second opinion can skip it.
    pub from_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    Item {
        item: TurnItem,
        /// The item's index in the turn's items.
        index: usize,
        view: ItemView,
        placement: Placement,
    },
    FoldLine(FoldLine),
    /// An orchestration proposal, appended after the lead's output.
    Proposal(BlockRef),
    /// The session accessory after the latest reply.
    Accessory,
    Footer(TurnFooter),
}

/// One list item.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Stable identity, unique in the transcript.
    pub key: String,
    pub turn_index: usize,
    /// The turn's first block id.
    pub turn_id: String,
    pub last_turn: bool,
    /// `data-transcript-search-current`.
    pub search_current: bool,
    pub kind: RowKind,
}

impl Row {
    /// Whether two rows draw the same thing. Blocks compare by identity,
    /// which [`BlockStore`] keeps stable for unchanged blocks.
    pub fn same_as(&self, other: &Row) -> bool {
        self.key == other.key
            && self.turn_index == other.turn_index
            && self.last_turn == other.last_turn
            && self.search_current == other.search_current
            && same_kind(&self.kind, &other.kind)
    }

    /// The first block the row draws, if any.
    pub fn first_block(&self) -> Option<&BlockRef> {
        match &self.kind {
            RowKind::Item { item, .. } => item.blocks().first(),
            RowKind::Proposal(block) => Some(block),
            _ => None,
        }
    }
}

fn same_blocks(a: &[BlockRef], b: &[BlockRef]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| Arc::ptr_eq(a, b))
}

fn same_item(a: &TurnItem, b: &TurnItem) -> bool {
    match (a, b) {
        (TurnItem::Block(a), TurnItem::Block(b)) => Arc::ptr_eq(a, b),
        (TurnItem::Activity(a), TurnItem::Activity(b))
        | (TurnItem::Subagents(a), TurnItem::Subagents(b)) => same_blocks(a, b),
        _ => false,
    }
}

fn same_kind(a: &RowKind, b: &RowKind) -> bool {
    match (a, b) {
        (
            RowKind::Item {
                item: a_item,
                index: a_index,
                view: a_view,
                placement: a_placement,
            },
            RowKind::Item {
                item: b_item,
                index: b_index,
                view: b_view,
                placement: b_placement,
            },
        ) => {
            a_index == b_index
                && a_view == b_view
                && a_placement == b_placement
                && same_item(a_item, b_item)
        }
        (RowKind::Proposal(a), RowKind::Proposal(b)) => Arc::ptr_eq(a, b),
        _ => a == b,
    }
}

/// Keeps one shared [`BlockRef`] per unchanged block across snapshots, so
/// rows and caches can compare blocks by pointer.
#[derive(Debug, Default)]
pub struct BlockStore {
    blocks: Vec<BlockRef>,
    by_id: HashMap<String, BlockRef>,
}

impl BlockStore {
    /// Take a new snapshot's blocks. Returns whether anything changed.
    pub fn update(&mut self, blocks: &[Block]) -> bool {
        let mut changed = blocks.len() != self.blocks.len();
        let mut next = Vec::with_capacity(blocks.len());
        let mut by_id = HashMap::with_capacity(blocks.len());
        for (index, block) in blocks.iter().enumerate() {
            let shared = match self.by_id.get(&block.id) {
                Some(previous) if **previous == *block => previous.clone(),
                _ => Arc::new(block.clone()),
            };
            if !changed && !Arc::ptr_eq(&shared, &self.blocks[index]) {
                changed = true;
            }
            by_id.insert(block.id.clone(), shared.clone());
            next.push(shared);
        }
        self.blocks = next;
        self.by_id = by_id;
        changed
    }

    pub fn blocks(&self) -> &[BlockRef] {
        &self.blocks
    }
}

/// The blocks the transcript shows: provider sign-in errors are handled by
/// the sign-in modal, so they are hidden for providers that have one.
pub fn visible_blocks(blocks: &[BlockRef], harness: Option<HarnessId>) -> Vec<BlockRef> {
    if !harness.is_some_and(supports_harness_login) {
        return blocks.to_vec();
    }
    blocks
        .iter()
        .filter(|block| {
            !(block.role == BlockRole::System
                && block.notice == Some(monocode_core::block::BlockNotice::Error)
                && is_harness_auth_error(&block.text))
        })
        .cloned()
        .collect()
}

/// Everything about a turn the layout reads besides its blocks. Equal flags
/// and equal blocks give equal rows, which is what [`PlanCache`] relies on.
#[derive(Debug, Clone, PartialEq)]
struct TurnFlags {
    turn_index: usize,
    is_last_turn: bool,
    settled: bool,
    visible: bool,
    live: bool,
    work_open: bool,
    search_current: Option<String>,
    turn_harness: Option<HarnessId>,
    current_model_name: Option<String>,
    waiting_for_approval: bool,
    waiting_label: Option<&'static str>,
    background: Vec<String>,
    has_accessory: bool,
    editable_user_block_id: Option<String>,
    can_edit_last_turn: bool,
    editing_last_turn: bool,
}

/// Rows of turns that have not changed since the last plan.
#[derive(Debug, Default)]
pub struct PlanCache {
    turns: HashMap<String, (Vec<BlockRef>, TurnFlags, Vec<Row>)>,
}

/// `harnessForTurn` for every turn in one pass: the recorded model wins, then
/// the last handoff before the turn, then the first handoff's source.
fn turn_harnesses(
    blocks: &[BlockRef],
    turns: &[Vec<BlockRef>],
    session: HarnessId,
) -> Vec<HarnessId> {
    let first_from = blocks
        .iter()
        .find_map(|block| block.handoff.as_ref())
        .map(|handoff| handoff.from);
    let position: HashMap<&str, usize> = blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id.as_str(), index))
        .collect();
    // The `to` of the last handoff at or before each block index.
    let mut last_to = Vec::with_capacity(blocks.len());
    let mut current = None;
    for block in blocks {
        if let Some(handoff) = &block.handoff {
            current = Some(handoff.to);
        }
        last_to.push(current);
    }
    turns
        .iter()
        .map(|turn| {
            if let Some(recorded) = turn
                .iter()
                .find(|block| block.role == BlockRole::User)
                .and_then(|block| block.turn_model.as_ref())
            {
                return recorded.harness;
            }
            let start = turn
                .first()
                .and_then(|first| position.get(first.id.as_str()))
                .copied();
            if let Some(start) = start
                && start > 0
                && let Some(to) = last_to[start - 1]
            {
                return to;
            }
            first_from.unwrap_or(session)
        })
        .collect()
}

/// Lay out every turn. `cache` reuses rows of turns whose blocks and flags
/// did not change, which during streaming is every turn but the last.
pub fn build_plan(
    blocks: &[BlockRef],
    options: &PlanOptions,
    state: &PlanState,
    cache: Option<&mut PlanCache>,
) -> Vec<Row> {
    let turns = group_turns(blocks, options.managed);
    let waiting_for_approval =
        blocks.iter().any(|block| needs_approval(block)) || options.pending_question;
    let preparing_handoff = blocks.iter().any(|block| {
        block.role == BlockRole::Handoff
            && block.handoff.as_ref().map(|handoff| handoff.status)
                == Some(monocode_core::block::HandoffStatus::Preparing)
    });
    let editable_user_block_id = last_user_turn_block(blocks).map(|block| block.id.clone());
    let harnesses = options
        .harness
        .map(|harness| turn_harnesses(blocks, &turns, harness));
    let waiting_label = if options.managed && waiting_for_approval {
        Some("Waiting for orchestrator")
    } else if options.pending_question {
        Some("Waiting for answers")
    } else {
        None
    };

    let mut fresh = PlanCache::default();
    let mut previous = cache;
    let mut rows = Vec::new();
    let count = turns.len();
    for (turn_index, turn) in turns.into_iter().enumerate() {
        let is_last_turn = turn_index + 1 == count;
        let settled = !(options.busy && is_last_turn);
        let turn_id = turn[0].id.clone();
        let live = options.visible && !settled && !preparing_handoff;
        let flags = TurnFlags {
            turn_index,
            is_last_turn,
            settled,
            visible: options.visible,
            live,
            work_open: state.open_work.get(&turn_id).copied().unwrap_or(false),
            search_current: state
                .search_current
                .clone()
                .filter(|id| turn.iter().any(|block| &block.id == id)),
            turn_harness: harnesses.as_ref().map(|all| all[turn_index]),
            current_model_name: if live {
                options.current_model_name.clone()
            } else {
                None
            },
            waiting_for_approval: live && waiting_for_approval,
            waiting_label: if live { waiting_label } else { None },
            background: if live {
                options.background_tasks.clone()
            } else {
                Vec::new()
            },
            has_accessory: is_last_turn && options.has_accessory,
            editable_user_block_id: editable_user_block_id
                .clone()
                .filter(|id| turn.iter().any(|block| &block.id == id)),
            can_edit_last_turn: options.can_edit_last_turn,
            editing_last_turn: options.editing_last_turn,
        };
        let cached = previous
            .as_deref_mut()
            .and_then(|cache| cache.turns.remove(&turn_id))
            .filter(|(blocks, cached_flags, _)| {
                same_blocks(blocks, &turn) && *cached_flags == flags
            });
        let turn_rows = match cached {
            Some((_, _, turn_rows)) => turn_rows,
            None => turn_rows(&turn, &flags, options.managed),
        };
        rows.extend(turn_rows.iter().cloned());
        fresh.turns.insert(turn_id, (turn, flags, turn_rows));
    }
    if let Some(cache) = previous {
        *cache = fresh;
    }
    rows
}

/// One turn's rows (the body of `visibleTurns.map` in AgentTranscript.tsx).
fn turn_rows(turn: &[BlockRef], flags: &TurnFlags, managed: bool) -> Vec<Row> {
    let turn_id = turn[0].id.clone();
    let user_block = turn_user_block(turn, managed);
    let duration_ms = user_block.and_then(|block| block.duration_ms);
    let settled = flags.settled;
    let proposals: Vec<BlockRef> = turn
        .iter()
        .filter(|block| block.orchestration.is_some())
        .cloned()
        .collect();
    // Proposals are turn results, like the changes card. Keep them out of
    // the live work and append them after all of the lead's output.
    let work: Vec<BlockRef> = turn
        .iter()
        .filter(|block| block.orchestration.is_none())
        .cloned()
        .collect();
    let items = group_turn_items(&work, settled);
    // Only the last activity group can still be the live one.
    let folded_at = last_activity_index(&items);
    let initial_thinking_at = initial_thinking_index(&items);
    let started_at = user_block.and_then(|block| block.started_at);
    // The agent starting its answer is the end of the work.
    let answering = folded_at.is_some_and(|at| {
        items[at + 1..]
            .iter()
            .any(|item| matches!(item, TurnItem::Block(block) if is_prose_block(block)))
    });
    let work_still_running = activity_still_running(turn);
    // New turns carry immutable model provenance. Legacy turns do not.
    let turn_model = user_block.and_then(|block| block.turn_model.as_ref());
    let fold = foldable_work(&items);
    let folded = fold
        .map(|fold| folded_blocks(&items, fold))
        .unwrap_or_default();
    let work_open = flags.work_open;
    let live = flags.live;
    let turn_model_name = turn_model
        .map(|model| model.name.clone())
        .or_else(|| flags.current_model_name.clone());
    // The fold line speaks for the main agent only.
    let fold_title = if live {
        FoldTitle::Live {
            started_at,
            paused: flags.waiting_for_approval,
            waiting_label: flags.waiting_label,
            background: flags.background.clone(),
            model_name: turn_model_name.clone(),
        }
    } else if let Some(duration) = duration_ms {
        FoldTitle::Text(format_working_duration(
            Some(duration),
            turn_model_name.as_deref(),
            true,
        ))
    } else {
        FoldTitle::Text(work_summary_line(&folded, false))
    };
    let show_fold_line = live || duration_ms.is_some() || fold.is_some();
    let first_work = first_foldable_index(&items);
    let fold_line_at = fold
        .map(|fold| fold.start)
        .or(first_work)
        .unwrap_or(items.len());
    let search_current = flags.search_current.as_deref();
    let is_current = |item: &TurnItem| {
        search_current.is_some_and(|current| item.blocks().iter().any(|block| block.id == current))
    };

    let item_view = |item: &TurnItem, index: usize| -> ItemView {
        match item {
            TurnItem::Subagents(_) => ItemView::Subagents { live },
            TurnItem::Activity(_) if Some(index) == initial_thinking_at => {
                ItemView::InitialThinking {
                    live: flags.visible && !settled,
                }
            }
            TurnItem::Activity(_) => ItemView::Activity {
                done: !flags.visible
                    || settled
                    || folded_at.is_some_and(|at| index < at)
                    || (answering && !work_still_running),
            },
            TurnItem::Block(block) => {
                let under_line = is_prose_block(block)
                    && index > 0
                    && (matches!(
                        items[index - 1],
                        TurnItem::Activity(_) | TurnItem::Subagents(_)
                    ) || (index == fold_line_at && show_fold_line));
                let editable = block.role == BlockRole::User
                    && flags.editable_user_block_id.as_deref() == Some(block.id.as_str());
                ItemView::Block {
                    under_line,
                    can_edit: flags.can_edit_last_turn && settled && editable && !block.is_draft(),
                    editing: flags.editing_last_turn && editable,
                }
            }
        }
    };

    let mut rows = Vec::new();
    let mut push = |key: String, search: bool, kind: RowKind| {
        rows.push(Row {
            key: format!("{turn_id}/{key}"),
            turn_index: flags.turn_index,
            turn_id: turn_id.clone(),
            last_turn: flags.is_last_turn,
            search_current: search,
            kind,
        });
    };
    let fold_line = FoldLine {
        title: fold_title,
        kind: work_kind(&folded),
        harness: flags.turn_harness,
        live,
        expandable: fold.is_some(),
        open: work_open && fold.is_some(),
    };
    let push_fold_line = |push: &mut dyn FnMut(String, bool, RowKind)| {
        if show_fold_line {
            push("fold".into(), false, RowKind::FoldLine(fold_line.clone()));
        }
    };

    for (index, item) in items.iter().enumerate() {
        let in_fold = fold.is_some_and(|fold| index >= fold.start && index <= fold.end);
        if let Some(fold) = fold.filter(|_| in_fold) {
            if index != fold.start {
                continue;
            }
            push_fold_line(&mut push);
            // The fold spans subagent stacks, but those rows stay out of its
            // body: they are parked under the work.
            let entries: Vec<(usize, &TurnItem)> =
                (fold.start..=fold.end).map(|at| (at, &items[at])).collect();
            if work_open {
                let body: Vec<&(usize, &TurnItem)> = entries
                    .iter()
                    .filter(|(_, entry)| !entry.is_subagents())
                    .collect();
                let last = body.len().saturating_sub(1);
                for (offset, (at, entry)) in body.iter().enumerate() {
                    let prose = matches!(entry, TurnItem::Block(block) if is_prose_block(block));
                    push(
                        format!("item/{}", entry.key()),
                        is_current(entry),
                        RowKind::Item {
                            item: (*entry).clone(),
                            index: *at,
                            view: item_view(entry, *at),
                            placement: Placement::FoldBody {
                                tail: offset == last,
                                prose,
                            },
                        },
                    );
                }
            }
            for (at, entry) in entries.iter().filter(|(_, entry)| entry.is_subagents()) {
                push(
                    format!("item/{}", entry.key()),
                    is_current(entry),
                    RowKind::Item {
                        item: (*entry).clone(),
                        index: *at,
                        view: item_view(entry, *at),
                        placement: Placement::FoldSubagents,
                    },
                );
            }
            continue;
        }
        if index == fold_line_at {
            push_fold_line(&mut push);
        }
        push(
            format!("item/{}", item.key()),
            is_current(item),
            RowKind::Item {
                item: item.clone(),
                index,
                view: item_view(item, index),
                placement: Placement::Plain,
            },
        );
    }
    if fold_line_at >= items.len() {
        push_fold_line(&mut push);
    }
    if settled {
        for block in proposals.iter().filter(|block| {
            block
                .orchestration
                .as_ref()
                .is_some_and(|proposal| proposal.status != OrchestrationProposalStatus::Planning)
        }) {
            push(
                format!("proposal/{}", block.id),
                false,
                RowKind::Proposal(block.clone()),
            );
        }
    }
    if flags.has_accessory {
        push("accessory".into(), false, RowKind::Accessory);
    }
    if let Some(elapsed_ms) = duration_ms.filter(|_| settled) {
        push(
            "footer".into(),
            false,
            RowKind::Footer(TurnFooter {
                elapsed_ms,
                metrics: user_block.and_then(|block| block.turn_metrics.clone()),
                label_hidden: show_fold_line,
                model_name: turn_model_name,
                completed_at: started_at.map(|started| started + elapsed_ms),
                copy_text: turn_copy_text(turn),
                harness: flags.turn_harness,
                from_model: turn_model.map(|model| model.id.clone()),
            }),
        );
    }
    rows
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
