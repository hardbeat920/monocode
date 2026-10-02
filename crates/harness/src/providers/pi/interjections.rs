//! Port of src/features/sessions/model/ompInterjections.ts: repair persisted
//! omp transcripts that older builds saved without their mid-turn
//! interjections, or with one answer split around a status row.
//!
//! The TypeScript returned the input array itself when nothing changed, and
//! its tests check that identity. Here that is `Cow::Borrowed`.
//!
//! The anchors and source texts come from the `omp_session_interjections` and
//! `omp_active_assistant_texts` commands in `monocode-git`. The types below
//! deserialize the JSON those commands write.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use monocode_core::block::{Block, BlockRole, InterjectionMeta, InterjectionSeverity};
use monocode_core::js;
use serde::{Deserialize, Serialize};

/// `OmpInterjectionAnchor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmpInterjectionAnchor {
    pub id: String,
    pub after_assistant_text: String,
    /// One-based occurrence among assistant messages with exactly this text.
    pub after_occurrence: i64,
    /// Direct-concat live representation, with its own exact-text occurrence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_assistant_text_concat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_concat_occurrence: Option<i64>,
    pub text: String,
    /// Full text of a directly following text-only answer, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub following_assistant_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub following_assistant_text_concat: Option<String>,
    pub custom_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<InterjectionSeverity>,
}

/// `OmpAssistantText`: one active-path assistant message in source order. Its
/// newline and concat forms are two spellings of one message, not two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmpAssistantText {
    pub text: String,
    pub concat: String,
}

fn is_status_row(block: &Block) -> bool {
    block.role == BlockRole::System && block.interjection.is_none()
}

/// `Object.keys(block)` holds only `id`, `role`, `text`, and `streaming`.
fn has_only_text_fields(block: &Block) -> bool {
    match serde_json::to_value(block) {
        Ok(serde_json::Value::Object(fields)) => fields
            .keys()
            .all(|key| matches!(key.as_str(), "id" | "role" | "text" | "streaming")),
        _ => false,
    }
}

/// `chainHops`: for each block index, the next fragment index in a
/// status-split chain (0 when none). Every row in between is a system row
/// that is not an interjection, and the next other row is a clean assistant
/// block.
fn chain_hops(blocks: &[Block]) -> Vec<usize> {
    let mut hop = vec![0; blocks.len()];
    let mut next_non_status = blocks.len();
    for index in (0..blocks.len()).rev() {
        let block = &blocks[index];
        if is_status_row(block) {
            continue;
        }
        let last = blocks.get(next_non_status);
        if block.role == BlockRole::Assistant
            && !block.text.is_empty()
            && !block.is_streaming()
            && next_non_status > index + 1
            && let Some(last) = last
            && last.role == BlockRole::Assistant
            && !last.text.is_empty()
            && !last.is_streaming()
            && has_only_text_fields(last)
        {
            hop[index] = next_non_status;
        }
        next_non_status = index;
    }
    hop
}

/// `ompStatusSplitTexts`: the joined text of every split chain whose trailing
/// fragments can be removed without losing data.
pub fn omp_status_split_texts(blocks: &[Block]) -> Vec<String> {
    let hop = chain_hops(blocks);
    let mut texts = Vec::new();
    let mut previous: Option<usize> = None;
    for (index, block) in blocks.iter().enumerate() {
        if is_status_row(block) {
            continue;
        }
        if hop[index] != 0 && previous.is_none_or(|previous| hop[previous] != index) {
            let mut text = block.text.clone();
            let mut next = hop[index];
            while next != 0 {
                text.push_str(&blocks[next].text);
                next = hop[next];
            }
            texts.push(text);
        }
        previous = Some(index);
    }
    texts
}

/// `sourcePositions`: the slots of every source text under both join forms.
/// One message fills one slot.
fn source_positions(source: &[OmpAssistantText]) -> HashMap<&str, Vec<usize>> {
    let mut positions: HashMap<&str, Vec<usize>> = HashMap::new();
    for (slot, message) in source.iter().enumerate() {
        positions
            .entry(message.text.as_str())
            .or_default()
            .push(slot);
        if message.concat != message.text {
            positions
                .entry(message.concat.as_str())
                .or_default()
                .push(slot);
        }
    }
    positions
}

/// `slotAt`: the first slot at or after `from` holding `text`, else `limit`.
fn slot_at(positions: &HashMap<&str, Vec<usize>>, text: &str, from: usize, limit: usize) -> usize {
    let Some(list) = positions.get(text) else {
        return limit;
    };
    let at = list.partition_point(|slot| *slot < from);
    list.get(at).copied().unwrap_or(limit)
}

/// `[items matched, consumed slot sum]`.
type AlignmentScore = (i64, i64);

fn beats(a: AlignmentScore, b: AlignmentScore) -> bool {
    a.0 > b.0 || (a.0 == b.0 && a.1 < b.1)
}

/// The alignment ran past its depth, memo, or text budget.
struct OverBudget;

/// The state the TypeScript closures in `mergeStatusSplits` shared.
struct Aligner<'a> {
    blocks: &'a [Block],
    hop: Vec<usize>,
    positions: HashMap<&'a str, Vec<usize>>,
    limit: usize,
    width: usize,
    max_text: usize,
    memo: HashMap<usize, AlignmentScore>,
    built: usize,
}

impl Aligner<'_> {
    fn next_slot(&self, text: &str, from: usize) -> usize {
        slot_at(&self.positions, text, from, self.limit)
    }

    fn bindable(block: &Block) -> bool {
        block.role == BlockRole::Assistant && !block.is_streaming() && !block.text.is_empty()
    }

    fn add_built(&mut self, text: &str) -> Result<(), OverBudget> {
        self.built += js::len(text);
        if self.built > 4_000_000 {
            return Err(OverBudget);
        }
        Ok(())
    }

    /// Best continuation score for `blocks[i..]` against `source[j..]`. Inert
    /// rows and single blocks are forced, so only split candidates branch.
    ///
    /// The TypeScript recursed, up to 2,000 calls deep before its budget
    /// stopped it. That depth can overflow a 512 KB worker stack in a debug
    /// build, so the calls live on an explicit stack of [`Frame`]s instead.
    /// The budgets, memo, and visiting order are unchanged.
    fn score(&mut self, i: usize, j: usize) -> Result<AlignmentScore, OverBudget> {
        let mut stack = vec![Frame::new(i, j)];
        let mut returned: AlignmentScore = (0, 0);
        loop {
            // Calls above this one: the TypeScript `depth` at its entry.
            let depth = stack.len() - 1;
            let frame = stack.last_mut().expect("a score frame");
            let step = match frame.phase {
                Phase::Start => self.start(frame, depth)?,
                Phase::AfterTail => {
                    let tail = returned;
                    frame.best = if frame.own < self.limit {
                        (tail.0 + 2, tail.1 + frame.own as i64)
                    } else {
                        tail
                    };
                    frame.combined = self.blocks[frame.i].text.clone();
                    frame.combined_len = js::len(&frame.combined);
                    frame.f = self.hop[frame.i];
                    frame.t = 1;
                    frame.phase = Phase::Welds;
                    Step::Continue
                }
                Phase::Welds => self.welds(frame)?,
                Phase::AfterRest => {
                    let rest = returned;
                    let candidate = (rest.0 + frame.t + 2, rest.1 + frame.merge_slot as i64);
                    if !beats(frame.best, candidate) {
                        frame.best = candidate;
                    }
                    frame.f = self.hop[frame.f];
                    frame.t += 1;
                    frame.phase = Phase::Welds;
                    Step::Continue
                }
            };
            match step {
                Step::Continue => {}
                Step::Call(i, j) => stack.push(Frame::new(i, j)),
                Step::Return(total) => {
                    stack.pop();
                    if stack.is_empty() {
                        return Ok(total);
                    }
                    returned = total;
                }
            }
        }
    }

    /// Entry of a `score` call: the memo, the budgets, then the forced scan
    /// up to the first split candidate.
    fn start(&mut self, frame: &mut Frame, depth: usize) -> Result<Step, OverBudget> {
        let key = frame.i * self.width + frame.j;
        if let Some(hit) = self.memo.get(&key) {
            return Ok(Step::Return(*hit));
        }
        if depth > 2_000 || self.memo.len() > 500_000 {
            return Err(OverBudget);
        }
        frame.key = key;
        let blocks = self.blocks;
        let (mut i, mut j) = (frame.i, frame.j);
        while i < blocks.len() {
            let block = &blocks[i];
            if !Self::bindable(block) {
                i += 1;
                continue;
            }
            let own = self.next_slot(&block.text, j);
            if self.hop[i] == 0 {
                if own < self.limit {
                    j = own + 1;
                    frame.matched += 2;
                    frame.consumed += own as i64;
                }
                i += 1;
                continue;
            }
            frame.i = i;
            frame.j = j;
            frame.own = own;
            frame.phase = Phase::AfterTail;
            return Ok(Step::Call(
                i + 1,
                if own < self.limit { own + 1 } else { j },
            ));
        }
        let total = (frame.matched, frame.consumed);
        self.memo.insert(key, total);
        Ok(Step::Return(total))
    }

    /// The weld loop over a candidate's fragments. Stops to score the rest
    /// after each valid weld.
    fn welds(&mut self, frame: &mut Frame) -> Result<Step, OverBudget> {
        let blocks = self.blocks;
        while frame.f != 0 && frame.combined_len <= self.max_text {
            let f = frame.f;
            frame.combined.push_str(&blocks[f].text);
            frame.combined_len += js::len(&blocks[f].text);
            self.add_built(&blocks[f].text)?;
            // A weld that lands behind the first fragment's own slot would
            // reorder evidence backward, so the earlier exact claim wins.
            let merge_slot = self.next_slot(&frame.combined, frame.j);
            if merge_slot < self.limit && (frame.own >= self.limit || merge_slot <= frame.own) {
                frame.merge_slot = merge_slot;
                frame.phase = Phase::AfterRest;
                return Ok(Step::Call(f + 1, merge_slot + 1));
            }
            frame.f = self.hop[f];
            frame.t += 1;
        }
        let total = (frame.matched + frame.best.0, frame.consumed + frame.best.1);
        self.memo.insert(frame.key, total);
        Ok(Step::Return(total))
    }
}

/// Where a suspended `score` call resumes.
#[derive(Clone, Copy)]
enum Phase {
    Start,
    AfterTail,
    Welds,
    AfterRest,
}

/// What the `score` loop does next.
enum Step {
    Continue,
    Call(usize, usize),
    Return(AlignmentScore),
}

/// One suspended `score` call and its locals.
struct Frame {
    key: usize,
    /// The split candidate, once the forced scan reaches it.
    i: usize,
    j: usize,
    matched: i64,
    consumed: i64,
    own: usize,
    best: AlignmentScore,
    combined: String,
    /// `combined.length`, in UTF-16 units.
    combined_len: usize,
    f: usize,
    t: i64,
    /// The weld slot whose remainder the current call scores.
    merge_slot: usize,
    phase: Phase,
}

impl Frame {
    fn new(i: usize, j: usize) -> Self {
        Self {
            key: 0,
            i,
            j,
            matched: 0,
            consumed: 0,
            own: 0,
            best: (0, 0),
            combined: String::new(),
            combined_len: 0,
            f: 0,
            t: 1,
            merge_slot: 0,
            phase: Phase::Start,
        }
    }
}

/// `mergeStatusSplits`: undo status-only splits through the best in-order
/// alignment between the persisted assistant sequence and the ordered
/// active-path source messages.
///
/// Every assistant block claims its earliest free source slot. A split
/// candidate may instead weld a prefix of its fragments into one slot by
/// their combined text. An alignment scores blocks plus source messages
/// matched, then the earliest consumed positions, decided per candidate
/// against the best continuation. A weld never wins when the fragments
/// explain themselves as real messages, including when the matching combined
/// text is a message the transcript never stored, and equal matches resolve
/// toward explaining earlier source slots. Anchors never authorize a merge.
/// Inputs past the alignment budget fail closed: the blocks come back
/// unmerged rather than welded by a weaker rule.
fn merge_status_splits<'a>(
    blocks: &'a [Block],
    source: &'a [OmpAssistantText],
) -> Cow<'a, [Block]> {
    let limit = source.len();
    let max_text = source
        .iter()
        .map(|message| js::len(&message.text).max(js::len(&message.concat)))
        .max()
        .unwrap_or(0);
    let mut aligner = Aligner {
        blocks,
        hop: chain_hops(blocks),
        positions: source_positions(source),
        limit,
        width: limit + 1,
        max_text,
        memo: HashMap::new(),
        built: 0,
    };
    match merge_with(&mut aligner) {
        Ok(Some(repaired)) => Cow::Owned(repaired),
        Ok(None) | Err(OverBudget) => Cow::Borrowed(blocks),
    }
}

struct Weld {
    last: usize,
    slot: usize,
    text: String,
}

fn merge_with(aligner: &mut Aligner<'_>) -> Result<Option<Vec<Block>>, OverBudget> {
    let blocks = aligner.blocks;
    let limit = aligner.limit;
    let mut cursor = 0;
    let mut repaired: Option<Vec<Block>> = None;
    let mut index = 0;
    while index < blocks.len() {
        let first = &blocks[index];
        if Aligner::bindable(first) && aligner.hop[index] != 0 {
            let own = aligner.next_slot(&first.text, cursor);
            let tail = aligner.score(index + 1, if own < limit { own + 1 } else { cursor })?;
            let mut best = if own < limit {
                (tail.0 + 2, tail.1 + own as i64)
            } else {
                tail
            };
            let mut chosen: Option<Weld> = None;
            let mut combined = first.text.clone();
            let mut combined_len = js::len(&combined);
            let mut f = aligner.hop[index];
            let mut t = 1;
            while f != 0 && combined_len <= aligner.max_text {
                combined.push_str(&blocks[f].text);
                combined_len += js::len(&blocks[f].text);
                aligner.add_built(&blocks[f].text)?;
                let merge_slot = aligner.next_slot(&combined, cursor);
                if merge_slot < limit && (own >= limit || merge_slot <= own) {
                    let rest = aligner.score(f + 1, merge_slot + 1)?;
                    let candidate = (rest.0 + t + 2, rest.1 + merge_slot as i64);
                    if !beats(best, candidate) {
                        best = candidate;
                        chosen = Some(Weld {
                            last: f,
                            slot: merge_slot,
                            text: combined.clone(),
                        });
                    }
                }
                f = aligner.hop[f];
                t += 1;
            }
            if let Some(chosen) = chosen {
                let out = repaired.get_or_insert_with(|| blocks[..index].to_vec());
                out.push(Block {
                    text: chosen.text,
                    ..first.clone()
                });
                out.extend(
                    blocks[index + 1..chosen.last]
                        .iter()
                        .filter(|block| block.role == BlockRole::System)
                        .cloned(),
                );
                cursor = chosen.slot + 1;
                index = chosen.last + 1;
                continue;
            }
            if own < limit {
                cursor = own + 1;
            }
        } else if Aligner::bindable(first) {
            let own = aligner.next_slot(&first.text, cursor);
            if own < limit {
                cursor = own + 1;
            }
        }
        if let Some(out) = repaired.as_mut() {
            out.push(first.clone());
        }
        index += 1;
    }
    Ok(repaired)
}

type NodeId = usize;

/// `BoundaryNode`: one row of the repaired transcript in a singly linked list.
struct BoundaryNode {
    block: Block,
    index: usize,
    offset: usize,
    next: Option<NodeId>,
}

/// The node arena and indexes `backfillOmpInterjections` keeps.
struct Boundaries {
    nodes: Vec<BoundaryNode>,
    positions: HashMap<String, Vec<NodeId>>,
}

impl Boundaries {
    fn source_order(&self, a: NodeId, b: NodeId) -> std::cmp::Ordering {
        let (a, b) = (&self.nodes[a], &self.nodes[b]);
        a.index.cmp(&b.index).then(a.offset.cmp(&b.offset))
    }

    fn add_position(&mut self, node: NodeId) {
        let text = self.nodes[node].block.text.clone();
        let mut matches = self.positions.remove(&text).unwrap_or_default();
        matches.push(node);
        if matches.len() > 1 && self.source_order(matches[matches.len() - 2], node).is_gt() {
            matches.sort_by(|a, b| self.source_order(*a, *b));
        }
        self.positions.insert(text, matches);
    }

    fn push(&mut self, node: BoundaryNode) -> NodeId {
        self.nodes.push(node);
        self.nodes.len() - 1
    }
}

/// `backfillOmpInterjections`: restore omitted boundaries, not turns, since
/// live interjections also stay mid-turn. Exact text and a one-based
/// occurrence keep it from inventing boundaries for progress prose. A
/// coalesced block splits only when both complete source messages
/// concatenate exactly to its text; prefixes alone are not evidence.
pub fn backfill_omp_interjections<'a>(
    blocks: &'a [Block],
    anchors: &[OmpInterjectionAnchor],
    source: &'a [OmpAssistantText],
) -> Cow<'a, [Block]> {
    let merged = merge_status_splits(blocks, source);
    if anchors.is_empty() {
        return merged;
    }
    let mut changed = matches!(merged, Cow::Owned(_));
    let mut list = Boundaries {
        nodes: Vec::new(),
        positions: HashMap::new(),
    };
    let mut ids: HashMap<String, NodeId> = HashMap::new();
    for (index, block) in merged.iter().enumerate() {
        list.push(BoundaryNode {
            block: block.clone(),
            index,
            offset: 0,
            next: None,
        });
    }
    let count = list.nodes.len();
    for node in 0..count {
        list.nodes[node].next = (node + 1 < count).then_some(node + 1);
        ids.insert(list.nodes[node].block.id.clone(), node);
        if list.nodes[node].block.role == BlockRole::Assistant {
            list.add_position(node);
        }
    }
    let mut matched_live: HashSet<NodeId> = HashSet::new();
    let mut boundary_ends: HashMap<NodeId, NodeId> = HashMap::new();
    let mut previous: Option<NodeId> = None;
    // A chain's final note alone has a directly linked continuation. Its exact
    // split evidence also locates earlier notes sealing that same source message.
    let mut continuations: HashMap<String, HashMap<i64, String>> = HashMap::new();
    let mut remember = |text: &str, occurrence: i64, following: Option<&str>| {
        let Some(following) = following.filter(|following| !following.is_empty()) else {
            return;
        };
        continuations
            .entry(text.to_string())
            .or_default()
            .insert(occurrence, following.to_string());
    };
    for anchor in anchors {
        remember(
            &anchor.after_assistant_text,
            anchor.after_occurrence,
            anchor.following_assistant_text.as_deref(),
        );
        if let Some(concat) = &anchor.after_assistant_text_concat {
            remember(
                concat,
                anchor
                    .after_concat_occurrence
                    .unwrap_or(anchor.after_occurrence),
                anchor.following_assistant_text_concat.as_deref(),
            );
        }
    }

    for anchor in anchors {
        // Keep legacy newline matches and ids; live streams concatenate text
        // parts. Each form has its own occurrence count, never fuzzy matching.
        let find = |list: &Boundaries, text: &str, occurrence: i64, following: Option<&str>| {
            let following = following
                .map(str::to_string)
                .or_else(|| continuations.get(text)?.get(&occurrence).cloned());
            let exact = list.positions.get(text).cloned().unwrap_or_default();
            let combined = match following.as_deref() {
                Some(following) if !following.is_empty() => list
                    .positions
                    .get(&format!("{text}{following}"))
                    .cloned()
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let candidates = if combined.is_empty() {
                exact
            } else {
                let mut all = exact;
                all.extend(combined);
                all.sort_by(|a, b| list.source_order(*a, *b));
                all
            };
            usize::try_from(occurrence - 1)
                .ok()
                .and_then(|at| candidates.get(at).copied())
        };
        let mut after_text = anchor.after_assistant_text.clone();
        let mut node = find(
            &list,
            &after_text,
            anchor.after_occurrence,
            anchor.following_assistant_text.as_deref(),
        );
        if node.is_none()
            && let Some(concat) = &anchor.after_assistant_text_concat
        {
            after_text = concat.clone();
            node = find(
                &list,
                concat,
                anchor
                    .after_concat_occurrence
                    .unwrap_or(anchor.after_occurrence),
                anchor.following_assistant_text_concat.as_deref(),
            );
        }
        let Some(node) = node else {
            continue;
        };
        if previous.is_some_and(|previous| list.source_order(node, previous).is_lt()) {
            continue;
        }
        previous = Some(node);
        let id = format!("omp-interjection-{}", anchor.id);
        let mut boundary = ids.get(&id).copied();
        // Newer builds already stored live interjections with random ids.
        // Match only the adjacent boundary, and use each live row at most once.
        if boundary.is_none() {
            let mut live = list.nodes[node].next;
            while let Some(candidate) = live {
                let block = &list.nodes[candidate].block;
                let Some(interjection) = block
                    .interjection
                    .as_ref()
                    .filter(|_| block.role == BlockRole::System)
                else {
                    break;
                };
                if !matched_live.contains(&candidate)
                    && !block.id.starts_with("omp-interjection-")
                    && block.text == anchor.text
                    && interjection.custom_type == anchor.custom_type
                    && interjection.severity == anchor.severity
                {
                    matched_live.insert(candidate);
                    boundary = Some(candidate);
                    break;
                }
                live = list.nodes[candidate].next;
            }
        }
        let boundary = match boundary {
            Some(boundary) => boundary,
            None => {
                let end = boundary_ends.get(&node).copied().unwrap_or(node);
                let mut block = Block::new(id.clone(), BlockRole::System, anchor.text.clone());
                block.interjection = Some(InterjectionMeta {
                    custom_type: anchor.custom_type.clone(),
                    severity: anchor.severity,
                    extra: Default::default(),
                });
                let created = list.push(BoundaryNode {
                    block,
                    index: list.nodes[node].index,
                    offset: list.nodes[node].offset,
                    next: list.nodes[end].next,
                });
                list.nodes[end].next = Some(created);
                ids.insert(id.clone(), created);
                changed = true;
                created
            }
        };
        boundary_ends.insert(node, boundary);
        if list.nodes[node].block.text != after_text {
            let text = list.nodes[node].block.text.clone();
            // `node` matched `after_text + following`, so `after_text` is a prefix.
            let split = after_text.len();
            let mut end = boundary;
            while let Some(next) = list.nodes[end].next {
                let block = &list.nodes[next].block;
                if block.role != BlockRole::System || block.interjection.is_none() {
                    break;
                }
                end = next;
            }
            let continuation_id = format!("{}-{id}-continuation", list.nodes[node].block.id);
            // The suffix belongs to this boundary, not to an insertion's old
            // index. Index it now so later anchors can target it in this pass.
            let continuation = list.push(BoundaryNode {
                block: Block::new(
                    continuation_id.clone(),
                    BlockRole::Assistant,
                    &text[split..],
                ),
                index: list.nodes[node].index,
                offset: list.nodes[node].offset + js::len(&after_text),
                next: list.nodes[end].next,
            });
            list.nodes[end].next = Some(continuation);
            if let Some(matches) = list.positions.get_mut(&text) {
                matches.retain(|candidate| *candidate != node);
            }
            list.nodes[node].block.text = after_text.clone();
            list.add_position(node);
            list.add_position(continuation);
            ids.insert(continuation_id, continuation);
            changed = true;
        }
    }
    if !changed {
        return Cow::Borrowed(blocks);
    }
    let mut repaired = Vec::new();
    let mut cursor = (count > 0).then_some(0);
    while let Some(node) = cursor {
        repaired.push(list.nodes[node].block.clone());
        cursor = list.nodes[node].next;
    }
    Cow::Owned(repaired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::block::BlockTool;

    fn anchor() -> OmpInterjectionAnchor {
        OmpInterjectionAnchor {
            id: "review".into(),
            after_assistant_text: "The complete answer.".into(),
            after_occurrence: 1,
            after_assistant_text_concat: None,
            after_concat_occurrence: None,
            text: "Check the fallback.".into(),
            following_assistant_text: None,
            following_assistant_text_concat: None,
            custom_type: "advisor".into(),
            severity: Some(InterjectionSeverity::Concern),
        }
    }

    fn block(id: &str, role: BlockRole, text: &str) -> Block {
        Block::new(id, role, text)
    }

    fn assistant(id: &str, text: &str) -> Block {
        block(id, BlockRole::Assistant, text)
    }

    fn system(id: &str, text: &str) -> Block {
        block(id, BlockRole::System, text)
    }

    fn note(id: &str, text: &str, severity: Option<InterjectionSeverity>) -> Block {
        let mut block = system(id, text);
        block.interjection = Some(InterjectionMeta {
            custom_type: "advisor".into(),
            severity,
            extra: Default::default(),
        });
        block
    }

    fn tool(id: &str, title: &str) -> Block {
        let mut block = block(id, BlockRole::Tool, title);
        block.tool = Some(BlockTool {
            kind: Some("shell".into()),
            title: Some(title.into()),
            status: Some("completed".into()),
            ..Default::default()
        });
        block
    }

    fn old_blocks() -> Vec<Block> {
        vec![
            block("u", BlockRole::User, "Go"),
            block("r1", BlockRole::Reasoning, "Thinking"),
            tool("t1", "Check"),
            assistant("a1", &anchor().after_assistant_text),
            block("r2", BlockRole::Reasoning, "Rechecking"),
            tool("t2", "Test"),
            assistant("a2", "Checked."),
        ]
    }

    fn ids(blocks: &[Block]) -> Vec<&str> {
        blocks.iter().map(|block| block.id.as_str()).collect()
    }

    fn texts(blocks: &[Block]) -> Vec<&str> {
        blocks.iter().map(|block| block.text.as_str()).collect()
    }

    fn id_texts(blocks: &[Block]) -> Vec<(&str, &str)> {
        blocks
            .iter()
            .map(|block| (block.id.as_str(), block.text.as_str()))
            .collect()
    }

    fn role_texts(blocks: &[Block]) -> Vec<(BlockRole, &str)> {
        blocks
            .iter()
            .map(|block| (block.role, block.text.as_str()))
            .collect()
    }

    fn interjection_ids(blocks: &[Block]) -> Vec<&str> {
        blocks
            .iter()
            .filter(|block| block.interjection.is_some())
            .map(|block| block.id.as_str())
            .collect()
    }

    fn unchanged(result: Cow<'_, [Block]>) -> bool {
        matches!(result, Cow::Borrowed(_))
    }

    fn backfill<'a>(blocks: &'a [Block], anchors: &[OmpInterjectionAnchor]) -> Cow<'a, [Block]> {
        backfill_omp_interjections(blocks, anchors, &[])
    }

    /// Ordered active-path source messages. A string is one message in both forms.
    fn source(messages: &[&str]) -> Vec<OmpAssistantText> {
        messages
            .iter()
            .map(|text| OmpAssistantText {
                text: text.to_string(),
                concat: text.to_string(),
            })
            .collect()
    }

    fn split(id: &str) -> Vec<Block> {
        vec![
            assistant(id, "First."),
            system(&format!("{id}-status"), "Reviewed"),
            assistant(&format!("{id}-end"), "Second."),
        ]
    }

    // The TypeScript tests also checked the transcript fold (`foldedIds`)
    // through transcriptActivity.ts, which is not ported yet. The placement
    // checks below cover the same repair.

    #[test]
    fn restores_the_complete_answer_outside_the_work_fold_without_inventing_a_turn() {
        let before = old_blocks();
        let repaired = backfill(&before, &[anchor()]);
        let boundary = &repaired[4];
        assert_eq!(boundary.id, "omp-interjection-review");
        assert_eq!(boundary.role, BlockRole::System);
        assert_eq!(boundary.text, anchor().text);
        let meta = boundary.interjection.as_ref().unwrap();
        assert_eq!(meta.custom_type, "advisor");
        assert_eq!(meta.severity, Some(InterjectionSeverity::Concern));
        assert_eq!(
            repaired
                .iter()
                .filter(|block| block.role == BlockRole::User)
                .count(),
            1
        );
        assert_eq!(ids(&before), ["u", "r1", "t1", "a1", "r2", "t2", "a2"]);
    }

    #[test]
    fn targets_the_second_identical_answer_rather_than_the_first() {
        let mut blocks = old_blocks();
        blocks[6].text = anchor().after_assistant_text;
        let repaired = backfill(
            &blocks,
            &[OmpInterjectionAnchor {
                after_occurrence: 2,
                ..anchor()
            }],
        );
        assert_eq!(
            ids(&repaired),
            [
                "u",
                "r1",
                "t1",
                "a1",
                "r2",
                "t2",
                "a2",
                "omp-interjection-review"
            ]
        );
    }

    #[test]
    fn keeps_unanchored_progress_prose_folding_and_rejects_approximate_matches() {
        let blocks = old_blocks();
        assert!(unchanged(backfill(&blocks, &[])));
        assert!(unchanged(backfill(
            &blocks,
            &[OmpInterjectionAnchor {
                after_assistant_text: "The complete".into(),
                ..anchor()
            }]
        )));
        assert!(unchanged(backfill(
            &blocks,
            &[OmpInterjectionAnchor {
                after_assistant_text: format!(" {}", anchor().after_assistant_text),
                ..anchor()
            }]
        )));
    }

    #[test]
    fn does_not_duplicate_repaired_or_already_captured_live_boundaries() {
        let repaired = backfill(&old_blocks(), &[anchor()]).into_owned();
        assert!(unchanged(backfill(&repaired, &[anchor()])));
        let live: Vec<Block> = repaired
            .iter()
            .map(|block| {
                if block.interjection.is_some() {
                    Block {
                        id: "random-live-id".into(),
                        ..block.clone()
                    }
                } else {
                    block.clone()
                }
            })
            .collect();
        assert!(unchanged(backfill(&live, &[anchor()])));
    }

    #[test]
    fn splits_a_coalesced_answer_only_with_an_exact_full_continuation_from_the_source() {
        let blocks = vec![assistant("a", "First.\u{1D11E}Second.")];
        let merged = OmpInterjectionAnchor {
            after_assistant_text: "First.\u{1D11E}".into(),
            following_assistant_text: Some("Second.".into()),
            ..anchor()
        };
        let repaired = backfill(&blocks, std::slice::from_ref(&merged)).into_owned();
        assert_eq!(
            role_texts(&repaired),
            [
                (BlockRole::Assistant, "First.\u{1D11E}"),
                (BlockRole::System, anchor().text.as_str()),
                (BlockRole::Assistant, "Second."),
            ]
        );
        assert!(unchanged(backfill(
            &repaired,
            std::slice::from_ref(&merged)
        )));
        assert!(unchanged(backfill(
            &blocks,
            &[OmpInterjectionAnchor {
                following_assistant_text: Some("Second".into()),
                ..merged.clone()
            }]
        )));
        assert!(unchanged(backfill(
            &blocks,
            &[OmpInterjectionAnchor {
                following_assistant_text: None,
                ..merged
            }]
        )));
    }

    #[test]
    fn preserves_source_order_and_consumes_live_rows_only_once() {
        let mut first = backfill(&old_blocks(), &[anchor()]).into_owned();
        first[4].id = "live".into();
        let second = OmpInterjectionAnchor {
            id: "review-again".into(),
            ..anchor()
        };
        let anchors = [anchor(), second];
        let repaired = backfill(&first, &anchors).into_owned();
        assert_eq!(
            interjection_ids(&repaired),
            ["live", "omp-interjection-review-again"]
        );
        assert!(unchanged(backfill(&repaired, &anchors)));
    }

    #[test]
    fn preserves_a_split_continuation_after_an_existing_live_boundary_and_a_new_anchor() {
        let blocks = vec![
            assistant("a", "First.Second."),
            note("live", &anchor().text, Some(InterjectionSeverity::Concern)),
        ];
        let first = OmpInterjectionAnchor {
            after_assistant_text: "First.Second.".into(),
            ..anchor()
        };
        let second = OmpInterjectionAnchor {
            id: "second".into(),
            after_assistant_text: "First.".into(),
            following_assistant_text: Some("Second.".into()),
            text: "Another review.".into(),
            ..anchor()
        };
        let anchors = [first, second];
        let repaired = backfill(&blocks, &anchors).into_owned();
        assert_eq!(
            role_texts(&repaired),
            [
                (BlockRole::Assistant, "First."),
                (BlockRole::System, anchor().text.as_str()),
                (BlockRole::System, "Another review."),
                (BlockRole::Assistant, "Second."),
            ]
        );
        assert!(unchanged(backfill(&repaired, &anchors)));
        assert_eq!(&*backfill(&blocks, &anchors), repaired.as_slice());
        assert_eq!(blocks[0].text, "First.Second.");
    }

    #[test]
    fn matches_subsequent_anchors_against_a_continuation_created_in_the_same_pass() {
        let blocks = vec![assistant("a", "First.Second.")];
        let first = OmpInterjectionAnchor {
            after_assistant_text: "First.".into(),
            following_assistant_text: Some("Second.".into()),
            ..anchor()
        };
        let second = OmpInterjectionAnchor {
            id: "second".into(),
            after_assistant_text: "Second.".into(),
            text: "Second review.".into(),
            ..anchor()
        };
        let first_text = first.text.clone();
        let anchors = [first, second];
        let repaired = backfill(&blocks, &anchors).into_owned();
        assert_eq!(
            role_texts(&repaired),
            [
                (BlockRole::Assistant, "First."),
                (BlockRole::System, first_text.as_str()),
                (BlockRole::Assistant, "Second."),
                (BlockRole::System, "Second review."),
            ]
        );
        assert!(unchanged(backfill(&repaired, &anchors)));
    }

    #[test]
    fn keeps_all_adjacent_live_notes_before_a_recovered_continuation() {
        let first = OmpInterjectionAnchor {
            after_assistant_text: "First.".into(),
            following_assistant_text: Some("Second.".into()),
            ..anchor()
        };
        let second = OmpInterjectionAnchor {
            id: "second".into(),
            text: "Second review.".into(),
            ..first.clone()
        };
        let blocks = vec![
            assistant("a", "First.Second."),
            note("live-1", &first.text, Some(InterjectionSeverity::Concern)),
            note("live-2", &second.text, Some(InterjectionSeverity::Concern)),
        ];
        let first_text = first.text.clone();
        let anchors = [first, second];
        let repaired = backfill(&blocks, &anchors).into_owned();
        assert_eq!(
            role_texts(&repaired),
            [
                (BlockRole::Assistant, "First."),
                (BlockRole::System, first_text.as_str()),
                (BlockRole::System, "Second review."),
                (BlockRole::Assistant, "Second."),
            ]
        );
        assert!(unchanged(backfill(&repaired, &anchors)));
    }

    #[test]
    fn adds_tool_result_and_chained_notes_around_an_already_repaired_boundary_without_changing_ids()
    {
        let before = backfill(&old_blocks(), &[anchor()]).into_owned();
        let earlier = OmpInterjectionAnchor {
            id: "tool-note".into(),
            text: "Tool review".into(),
            ..anchor()
        };
        let later = OmpInterjectionAnchor {
            id: "chained-note".into(),
            text: "Another review".into(),
            ..anchor()
        };
        let anchors = [earlier, anchor(), later];
        let repaired = backfill(&before, &anchors).into_owned();
        assert_eq!(
            interjection_ids(&repaired),
            [
                "omp-interjection-tool-note",
                "omp-interjection-review",
                "omp-interjection-chained-note"
            ]
        );
        let plain: Vec<Block> = repaired
            .iter()
            .filter(|block| block.interjection.is_none())
            .cloned()
            .collect();
        assert_eq!(plain, old_blocks());
        assert!(unchanged(backfill(&repaired, &anchors)));
    }

    #[test]
    fn matches_both_multipart_representations_with_separate_occurrences_and_exact_split_evidence() {
        let multipart = OmpInterjectionAnchor {
            after_assistant_text: "One\nTwo".into(),
            after_assistant_text_concat: Some("OneTwo".into()),
            after_occurrence: 1,
            after_concat_occurrence: Some(2),
            following_assistant_text: Some("Three\nFour".into()),
            following_assistant_text_concat: Some("ThreeFour".into()),
            ..anchor()
        };
        let blocks = vec![
            assistant("earlier", "OneTwo"),
            assistant("target", "OneTwoThreeFour"),
        ];
        let repaired = backfill(&blocks, std::slice::from_ref(&multipart)).into_owned();
        assert_eq!(
            texts(&repaired),
            ["OneTwo", "OneTwo", anchor().text.as_str(), "ThreeFour"]
        );
        assert!(unchanged(backfill(
            &repaired,
            std::slice::from_ref(&multipart)
        )));
        let legacy = vec![assistant("target", "One\nTwoThree\nFour")];
        assert_eq!(
            texts(&backfill(&legacy, std::slice::from_ref(&multipart))),
            ["One\nTwo", anchor().text.as_str(), "Three\nFour"]
        );
        assert!(unchanged(backfill(
            &blocks,
            &[OmpInterjectionAnchor {
                following_assistant_text_concat: Some("Three".into()),
                ..multipart
            }]
        )));
    }

    #[test]
    fn restores_an_entire_note_chain_when_only_its_last_note_has_exact_split_evidence() {
        let first = OmpInterjectionAnchor {
            after_assistant_text: "First.".into(),
            ..anchor()
        };
        let last = OmpInterjectionAnchor {
            id: "last".into(),
            text: "Last note".into(),
            following_assistant_text: Some("Second.".into()),
            ..first.clone()
        };
        let blocks = vec![assistant("a", "First.Second.")];
        let first_text = first.text.clone();
        let anchors = [first, last];
        let repaired = backfill(&blocks, &anchors).into_owned();
        assert_eq!(
            texts(&repaired),
            ["First.", first_text.as_str(), "Last note", "Second."]
        );
        assert!(unchanged(backfill(&repaired, &anchors)));
    }

    #[test]
    fn merges_status_split_prose_with_the_next_source_message_before_placing_later_anchors() {
        let first = OmpInterjectionAnchor {
            after_assistant_text: "First.Second.".into(),
            following_assistant_text: Some("Later.".into()),
            ..anchor()
        };
        let later = OmpInterjectionAnchor {
            id: "later".into(),
            after_assistant_text: "Later.".into(),
            ..anchor()
        };
        let blocks = vec![
            assistant("a", "First."),
            system("status", "Advisor reviewed this turn"),
            assistant("b", "Second."),
            assistant("c", "Later."),
        ];
        let messages = source(&["First.Second.", "Later."]);
        let anchors = [first.clone(), later];
        let repaired = backfill_omp_interjections(&blocks, &anchors, &messages).into_owned();
        assert_eq!(
            id_texts(&repaired),
            [
                ("a", "First.Second."),
                ("omp-interjection-review", anchor().text.as_str()),
                ("status", "Advisor reviewed this turn"),
                ("c", "Later."),
                ("omp-interjection-later", anchor().text.as_str()),
            ]
        );
        assert!(unchanged(backfill_omp_interjections(
            &repaired, &anchors, &messages
        )));
        assert_eq!(blocks[0].text, "First.");
        assert!(unchanged(backfill_omp_interjections(
            &blocks,
            &[OmpInterjectionAnchor {
                after_assistant_text: "First. Second.".into(),
                ..first.clone()
            }],
            &source(&["First. Second.", "Later."])
        )));
        let interleaved: Vec<Block> = blocks
            .iter()
            .map(|block| {
                if block.id == "status" {
                    let mut block = block.clone();
                    block.interjection = Some(InterjectionMeta {
                        custom_type: "advisor".into(),
                        severity: None,
                        extra: Default::default(),
                    });
                    block
                } else {
                    block.clone()
                }
            })
            .collect();
        assert!(unchanged(backfill_omp_interjections(
            &interleaved,
            std::slice::from_ref(&first),
            &messages
        )));
        let metadata: Vec<Block> = blocks
            .iter()
            .map(|block| {
                if block.id == "b" {
                    Block {
                        duration_ms: Some(10),
                        ..block.clone()
                    }
                } else {
                    block.clone()
                }
            })
            .collect();
        assert!(unchanged(backfill_omp_interjections(
            &metadata,
            &[first],
            &messages
        )));
    }

    #[test]
    fn never_merges_on_anchor_occurrences_alone() {
        let blocks = vec![
            assistant("a", "First."),
            system("status", "Reviewed"),
            assistant("b", "Second."),
        ];
        let anchors = [OmpInterjectionAnchor {
            after_assistant_text: "First.Second.".into(),
            after_assistant_text_concat: Some("First.Second.".into()),
            after_concat_occurrence: Some(1),
            ..anchor()
        }];
        assert!(unchanged(backfill(&blocks, &anchors)));
        assert!(unchanged(backfill(
            &blocks,
            &[OmpInterjectionAnchor {
                after_assistant_text: "First.\nSecond.".into(),
                after_occurrence: 2,
                ..anchors[0].clone()
            }]
        )));
        let messages = source(&["First.Second."]);
        let repaired = backfill_omp_interjections(&blocks, &anchors, &messages);
        assert_eq!(ids(&repaired), ["a", "omp-interjection-review", "status"]);
    }

    #[test]
    fn keeps_a_split_whose_fragments_are_the_next_two_source_messages_before_the_real_complete_answer()
     {
        let blocks = vec![
            assistant("a", "First."),
            system("status", "Reviewed"),
            assistant("b", "Second."),
            assistant("c", "First.Second."),
        ];
        let messages = source(&["First.", "Second.", "First.Second."]);
        assert_eq!(omp_status_split_texts(&blocks), ["First.Second."]);
        assert!(unchanged(backfill_omp_interjections(
            &blocks,
            &[],
            &messages
        )));
        let anchors = [OmpInterjectionAnchor {
            after_assistant_text: "First.Second.".into(),
            ..anchor()
        }];
        let repaired = backfill_omp_interjections(&blocks, &anchors, &messages).into_owned();
        assert_eq!(
            ids(&repaired),
            ["a", "status", "b", "c", "omp-interjection-review"]
        );
        assert!(unchanged(backfill_omp_interjections(
            &repaired, &anchors, &messages
        )));
    }

    #[test]
    fn keeps_a_split_whose_fragments_are_later_source_messages_when_the_combined_text_was_never_persisted()
     {
        let blocks = split("a");
        let messages = source(&["First.Second.", "First.", "Second."]);
        assert!(unchanged(backfill_omp_interjections(
            &blocks,
            &[],
            &messages
        )));
        let mut tail = blocks.clone();
        tail.push(assistant("c", "First.Second."));
        let messages_tail = source(&["First.Second.", "First.", "Second.", "First.Second."]);
        assert!(unchanged(backfill_omp_interjections(
            &tail,
            &[],
            &messages_tail
        )));
    }

    #[test]
    fn prefers_the_alignment_that_explains_every_block_when_fragment_slots_are_claimed_by_later_pairs()
     {
        let mut blocks = split("p1");
        blocks.push(assistant("m", "Marker."));
        blocks.extend(split("p2"));
        blocks.push(assistant("tail", "First.Second."));
        let messages = source(&[
            "First.Second.",
            "Marker.",
            "First.",
            "Second.",
            "First.Second.",
        ]);
        let repaired = backfill_omp_interjections(&blocks, &[], &messages);
        assert_eq!(
            id_texts(&repaired),
            [
                ("p1", "First.Second."),
                ("p1-status", "Reviewed"),
                ("m", "Marker."),
                ("p2", "First."),
                ("p2-status", "Reviewed"),
                ("p2-end", "Second."),
                ("tail", "First.Second."),
            ]
        );
    }

    fn chained() -> Vec<Block> {
        vec![
            assistant("a", "A."),
            system("a-s1", "Reviewed"),
            assistant("a-b", "B."),
            system("a-s2", "Reviewed"),
            assistant("a-c", "C."),
        ]
    }

    #[test]
    fn repairs_a_status_chain_as_one_message_in_a_single_idempotent_pass() {
        let blocks = chained();
        let messages = source(&["A.B.", "A.B.C."]);
        let repaired = backfill_omp_interjections(&blocks, &[], &messages).into_owned();
        assert_eq!(
            id_texts(&repaired),
            [("a", "A.B.C."), ("a-s1", "Reviewed"), ("a-s2", "Reviewed")]
        );
        assert!(unchanged(backfill_omp_interjections(
            &repaired,
            &[],
            &messages
        )));
    }

    #[test]
    fn welds_only_the_pair_when_a_chains_middle_fragment_ends_a_real_message() {
        let blocks = chained();
        let messages = source(&["A.B.", "C."]);
        let repaired = backfill_omp_interjections(&blocks, &[], &messages);
        assert_eq!(
            id_texts(&repaired),
            [
                ("a", "A.B."),
                ("a-s1", "Reviewed"),
                ("a-s2", "Reviewed"),
                ("a-c", "C.")
            ]
        );
    }

    #[test]
    fn never_re_welds_a_block_that_already_claimed_an_earlier_source_slot() {
        let blocks = vec![
            assistant("a", "First."),
            system("a-status", "Reviewed"),
            assistant("b", "Second."),
            assistant("c", "Third."),
        ];
        let messages = source(&["First.Second.", "First.Second.Third."]);
        let once = backfill_omp_interjections(&blocks, &[], &messages).into_owned();
        assert_eq!(
            id_texts(&once),
            [
                ("a", "First.Second."),
                ("a-status", "Reviewed"),
                ("c", "Third.")
            ]
        );
        assert!(unchanged(backfill_omp_interjections(&once, &[], &messages)));
    }

    #[test]
    fn keeps_a_split_whose_fragments_are_separated_later_source_messages() {
        let blocks = split("a");
        let messages = source(&["First.Second.", "Unrelated.", "First.", "Second."]);
        assert!(unchanged(backfill_omp_interjections(
            &blocks,
            &[],
            &messages
        )));
    }

    #[test]
    fn still_merges_a_true_split_when_only_one_fragment_is_a_later_source_message() {
        for messages in [
            source(&["First.Second.", "First."]),
            source(&["First.Second.", "Second."]),
        ] {
            let blocks = split("a");
            let repaired = backfill_omp_interjections(&blocks, &[], &messages);
            assert_eq!(texts(&repaired), ["First.Second.", "Reviewed"]);
        }
    }

    #[test]
    fn leaves_a_split_whose_first_fragment_is_its_own_source_message_and_anchors_the_later_complete_answer()
     {
        let blocks = vec![
            assistant("a", "The complete "),
            system("status", "Reviewed"),
            assistant("b", "answer."),
            assistant("c", &anchor().after_assistant_text),
        ];
        let messages = source(&["The complete ", "answer.", &anchor().after_assistant_text]);
        let repaired = backfill_omp_interjections(&blocks, &[anchor()], &messages).into_owned();
        assert_eq!(
            ids(&repaired),
            ["a", "status", "b", "c", "omp-interjection-review"]
        );
        assert!(unchanged(backfill_omp_interjections(
            &repaired,
            &[anchor()],
            &messages
        )));
        assert!(unchanged(backfill_omp_interjections(
            &blocks,
            &[OmpInterjectionAnchor {
                after_occurrence: 2,
                ..anchor()
            }],
            &messages
        )));
    }

    #[test]
    fn keeps_streaming_fragments_and_metadata_even_with_a_matching_source_message() {
        let blocks = vec![
            assistant("a", "First."),
            system("status", "Reviewed"),
            assistant("b", "Second."),
        ];
        let messages = source(&["First.Second."]);
        for index in [0, 2] {
            let mut streaming = blocks.clone();
            streaming[index].streaming = Some(true);
            assert!(omp_status_split_texts(&streaming).is_empty());
            assert!(unchanged(backfill_omp_interjections(
                &streaming,
                &[],
                &messages
            )));
        }
        let mut metadata = blocks.clone();
        metadata[2].duration_ms = Some(10);
        assert!(omp_status_split_texts(&metadata).is_empty());
        assert!(unchanged(backfill_omp_interjections(
            &metadata,
            &[],
            &messages
        )));
    }

    #[test]
    fn consumes_one_source_message_for_one_split_in_either_form() {
        for message in [
            OmpAssistantText {
                text: "First.\nSecond.".into(),
                concat: "First.Second.".into(),
            },
            OmpAssistantText {
                text: "First.Second.".into(),
                concat: "First.Second.".into(),
            },
        ] {
            let mut blocks = split("a");
            blocks.extend(split("b"));
            let messages = vec![message];
            let repaired = backfill_omp_interjections(&blocks, &[], &messages).into_owned();
            assert_eq!(
                repaired.iter().find(|block| block.id == "a").unwrap().text,
                "First.Second."
            );
            let b: Vec<Block> = repaired
                .iter()
                .filter(|block| block.id.starts_with('b'))
                .cloned()
                .collect();
            assert_eq!(b, split("b"));
            assert!(unchanged(backfill_omp_interjections(
                &repaired,
                &[],
                &messages
            )));
        }
    }

    #[test]
    fn merges_a_true_split_followed_by_a_different_complete_message() {
        let mut blocks = split("a");
        blocks.push(assistant("c", "Later."));
        let messages = source(&["First.Second.", "Later."]);
        let repaired = backfill_omp_interjections(&blocks, &[], &messages);
        assert_eq!(
            id_texts(&repaired),
            [
                ("a", "First.Second."),
                ("a-status", "Reviewed"),
                ("c", "Later.")
            ]
        );
    }

    #[test]
    fn lets_an_unsplit_answer_occupy_its_source_slot_before_a_later_candidate() {
        for anchored in [false, true] {
            let mut blocks = vec![assistant("complete", "First.Second.")];
            blocks.extend(split("a"));
            let anchors = if anchored {
                vec![OmpInterjectionAnchor {
                    after_assistant_text: "First.Second.".into(),
                    ..anchor()
                }]
            } else {
                Vec::new()
            };
            let one = source(&["First.Second."]);
            let repaired = backfill_omp_interjections(&blocks, &anchors, &one);
            let a: Vec<Block> = repaired
                .iter()
                .filter(|block| block.id.starts_with('a'))
                .cloned()
                .collect();
            assert_eq!(a, split("a"), "anchored: {anchored}");
            let two = source(&["First.Second.", "First.Second."]);
            let both = backfill_omp_interjections(&blocks, &anchors, &two);
            let assistants: Vec<(&str, &str)> = both
                .iter()
                .filter(|block| block.role == BlockRole::Assistant)
                .map(|block| (block.id.as_str(), block.text.as_str()))
                .collect();
            assert_eq!(
                assistants,
                [("complete", "First.Second."), ("a", "First.Second.")],
                "anchored: {anchored}"
            );
        }
    }

    #[test]
    fn repairs_two_splits_only_with_two_source_messages() {
        let mut blocks = split("a");
        blocks.extend(split("b"));
        let messages = source(&["First.Second.", "First.Second."]);
        let repaired = backfill_omp_interjections(&blocks, &[], &messages).into_owned();
        assert_eq!(ids(&repaired), ["a", "a-status", "b", "b-status"]);
        let assistants: Vec<&str> = repaired
            .iter()
            .filter(|block| block.role == BlockRole::Assistant)
            .map(|block| block.text.as_str())
            .collect();
        assert_eq!(assistants, ["First.Second.", "First.Second."]);
        assert!(unchanged(backfill_omp_interjections(
            &repaired,
            &[],
            &messages
        )));
    }

    #[test]
    fn keeps_anchor_evidence_from_adding_to_the_source_sequence() {
        let mut blocks = split("a");
        blocks.extend(split("b"));
        let anchors = [OmpInterjectionAnchor {
            after_assistant_text: "First.Second.".into(),
            ..anchor()
        }];
        let messages = source(&["First.Second."]);
        let repaired = backfill_omp_interjections(&blocks, &anchors, &messages).into_owned();
        let b: Vec<Block> = repaired
            .iter()
            .filter(|block| block.id.starts_with('b'))
            .cloned()
            .collect();
        assert_eq!(b, split("b"));
        assert!(unchanged(backfill_omp_interjections(
            &repaired, &anchors, &messages
        )));
    }

    #[test]
    fn does_not_bind_a_later_anchor_occurrence_or_unnumbered_following_text_to_an_earlier_split() {
        let blocks = split("a");
        assert!(unchanged(backfill(
            &blocks,
            &[OmpInterjectionAnchor {
                after_assistant_text: "First.Second.".into(),
                after_occurrence: 2,
                ..anchor()
            }]
        )));
        assert!(unchanged(backfill(
            &blocks,
            &[OmpInterjectionAnchor {
                following_assistant_text: Some("First.Second.".into()),
                ..anchor()
            }]
        )));
    }

    #[test]
    fn merges_ten_identical_splits_only_against_the_three_source_messages_in_order() {
        let blocks: Vec<Block> = (0..10)
            .flat_map(|index| split(&index.to_string()))
            .collect();
        let messages = source(&["First.Second.", "First.Second.", "First.Second."]);
        let repaired = backfill_omp_interjections(&blocks, &[], &messages).into_owned();
        let merged: Vec<&str> = repaired
            .iter()
            .filter(|block| block.role == BlockRole::Assistant && block.text == "First.Second.")
            .map(|block| block.id.as_str())
            .collect();
        assert_eq!(merged, ["0", "1", "2"]);
        assert_eq!(
            repaired
                .iter()
                .filter(|block| block.id.ends_with("-end"))
                .count(),
            7
        );
        assert!(unchanged(backfill_omp_interjections(
            &repaired,
            &[],
            &messages
        )));
    }

    #[test]
    fn realigns_a_complete_block_past_source_messages_the_transcript_never_stored() {
        let mut blocks = vec![assistant("complete", "Intro.")];
        blocks.extend(split("a"));
        let messages = source(&["Setup.", "Intro.", "First.Second."]);
        let repaired = backfill_omp_interjections(&blocks, &[], &messages);
        assert_eq!(ids(&repaired), ["complete", "a", "a-status"]);
    }

    #[test]
    fn returns_blocks_unmerged_when_the_alignment_exceeds_its_budget() {
        let chain = |count: usize| -> Vec<Block> {
            let mut blocks = Vec::new();
            for index in 0..count {
                blocks.push(assistant(&format!("f{index}"), &format!("p{index}.")));
                if index + 1 < count {
                    blocks.push(system(&format!("f{index}-status"), "Reviewed"));
                }
            }
            blocks
        };
        let joined = |count: usize| {
            (0..count)
                .map(|index| format!("p{index}."))
                .collect::<String>()
        };
        // Control: a short chain of the same shape welds into one assistant block.
        let short = chain(3);
        let messages = source(&[&joined(3)]);
        let merged = backfill_omp_interjections(&short, &[], &messages);
        assert_eq!(
            id_texts(&merged),
            [
                ("f0", joined(3).as_str()),
                ("f0-status", "Reviewed"),
                ("f1-status", "Reviewed")
            ]
        );
        // 2,500 fragments drive the alignment past its depth budget.
        // The persisted repair must fail closed, not weld by a weaker rule.
        let deep = chain(2_500);
        let messages = source(&[&joined(2_500)]);
        // A 256 KB stack checks that the alignment no longer recurses.
        let result = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                let unchanged = unchanged(backfill_omp_interjections(&deep, &[], &messages));
                (unchanged, deep)
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(result.0);
        assert_eq!(result.1, chain(2_500));
    }

    #[test]
    fn reads_the_anchor_json_the_git_crate_writes() {
        let anchor: OmpInterjectionAnchor = serde_json::from_value(serde_json::json!({
            "id": "x",
            "afterAssistantText": "a",
            "afterOccurrence": 1,
            "afterAssistantTextConcat": "a",
            "afterConcatOccurrence": 1,
            "followingAssistantText": null,
            "followingAssistantTextConcat": null,
            "text": "note",
            "customType": "advisor",
            "severity": null,
        }))
        .unwrap();
        assert_eq!(anchor.following_assistant_text, None);
        assert_eq!(anchor.severity, None);
    }
}
