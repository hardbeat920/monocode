import type { OmpAssistantText, OmpInterjectionAnchor } from "./fs";
import type { Block } from "./session";

interface BoundaryNode {
  block: Block;
  index: number;
  offset: number;
  next?: BoundaryNode;
}

function sourceOrder(a: BoundaryNode, b: BoundaryNode): number {
  return a.index - b.index || a.offset - b.offset;
}

/** For each block index, the next fragment index in a status-split chain
 * (0 when none): every intervening row is a non-interjection system row and
 * the next other row is a clean assistant block. */
function chainHops(blocks: Block[]): Int32Array {
  const hop = new Int32Array(blocks.length);
  let nextNonStatus = blocks.length;
  for (let index = blocks.length - 1; index >= 0; index--) {
    const block = blocks[index];
    if (block.role === "system" && !block.interjection) continue;
    const last = nextNonStatus < blocks.length ? blocks[nextNonStatus] : undefined;
    if (
      block.role === "assistant" && block.text && !block.streaming &&
      nextNonStatus > index + 1 && last?.role === "assistant" && last.text &&
      !last.streaming &&
      Object.keys(last).every(key => ["id", "role", "text", "streaming"].includes(key))
    ) hop[index] = nextNonStatus;
    nextNonStatus = index;
  }
  return hop;
}

/** Collect only shapes whose trailing fragments can be removed without data loss. */
export function ompStatusSplitTexts(blocks: Block[]): string[] {
  const hop = chainHops(blocks);
  const texts: string[] = [];
  let previous = -1;
  for (let index = 0; index < blocks.length; index++) {
    const block = blocks[index];
    if (block.role === "system" && !block.interjection) continue;
    if (hop[index] && (previous < 0 || hop[previous] !== index)) {
      let text = block.text;
      for (let next = hop[index]; next; next = hop[next]) text += blocks[next].text;
      texts.push(text);
    }
    previous = index;
  }
  return texts;
}

/** Positions of every source text under both join forms; one message, one slot. */
function sourcePositions(source: readonly OmpAssistantText[]): Map<string, number[]> {
  const positions = new Map<string, number[]>();
  source.forEach((message, slot) => {
    const list = positions.get(message.text);
    if (list) list.push(slot);
    else positions.set(message.text, [slot]);
    if (message.concat !== message.text) {
      const concatList = positions.get(message.concat);
      if (concatList) concatList.push(slot);
      else positions.set(message.concat, [slot]);
    }
  });
  return positions;
}

function slotAt(positions: Map<string, number[]>, text: string, from: number, limit: number): number {
  const list = positions.get(text);
  if (!list) return limit;
  let lo = 0;
  let hi = list.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (list[mid] < from) lo = mid + 1;
    else hi = mid;
  }
  return lo < list.length ? list[lo] : limit;
}

type AlignmentScore = readonly [number, number]; // [items matched, consumed slot sum]

function beats(a: AlignmentScore, b: AlignmentScore): boolean {
  return a[0] > b[0] || (a[0] === b[0] && a[1] < b[1]);
}

/** Undo status-only splits through the best in-order alignment between the
 * persisted assistant sequence and the ordered active-path source messages.
 * Every assistant block claims its earliest free source slot; a split
 * candidate may instead weld a prefix of its fragments into one slot by
 * their combined text. An alignment scores (blocks + source messages
 * matched, then earliest consumed positions), decided per candidate against
 * the optimal continuation: a weld never wins when the fragments explain
 * themselves as real messages — including when the matching combined text is
 * a message the transcript simply never stored — and equal matches resolve
 * toward explaining earlier source slots. Anchors never authorize a merge.
 * Pathological inputs fall back to the cursor rule.
 */
function mergeStatusSplits(blocks: Block[], source: readonly OmpAssistantText[]): Block[] {
  const limit = source.length;
  const width = limit + 1;
  const positions = sourcePositions(source);
  const maxText = source.reduce(
    (max, message) => Math.max(max, message.text.length, message.concat.length), 0);
  const nextSlot = (text: string, from: number) => slotAt(positions, text, from, limit);
  const bindable = (block: Block) =>
    block.role === "assistant" && !block.streaming && !!block.text;
  const hop = chainHops(blocks);
  try {
    // Best continuation score for blocks[i..] against source[j..]: inert
    // rows and single blocks are forced, so only split candidates branch.
    const memo = new Map<number, AlignmentScore>();
    let depth = 0;
    let built = 0;
    const score = (i: number, j: number): AlignmentScore => {
      const key = i * width + j;
      const hit = memo.get(key);
      if (hit !== undefined) return hit;
      if (depth > 2_000 || memo.size > 500_000) throw new Error("alignment budget");
      depth++;
      try {
        let matched = 0;
        let consumed = 0;
        while (i < blocks.length) {
          const block = blocks[i];
          if (!bindable(block)) {
            i++;
            continue;
          }
          if (!hop[i]) {
            const own = nextSlot(block.text, j);
            if (own < limit) {
              j = own + 1;
              matched += 2;
              consumed += own;
            }
            i++;
            continue;
          }
          const own = nextSlot(block.text, j);
          const tail = score(i + 1, own < limit ? own + 1 : j);
          let best: AlignmentScore = own < limit
            ? [tail[0] + 2, tail[1] + own]
            : tail;
          let combined = block.text;
          for (let f = hop[i], t = 1; f && combined.length <= maxText; f = hop[f], t++) {
            combined += blocks[f].text;
            built += blocks[f].text.length;
            if (built > 4_000_000) throw new Error("alignment budget");
            // A weld that lands behind the first fragment's own slot would
            // reorder evidence backward — the earlier exact claim wins.
            const mergeSlot = nextSlot(combined, j);
            if (mergeSlot < limit && (own >= limit || mergeSlot <= own)) {
              const rest = score(f + 1, mergeSlot + 1);
              const candidate: AlignmentScore = [rest[0] + t + 2, rest[1] + mergeSlot];
              if (!beats(best, candidate)) best = candidate;
            }
          }
          const total: AlignmentScore = [matched + best[0], consumed + best[1]];
          memo.set(key, total);
          return total;
        }
        const total: AlignmentScore = [matched, consumed];
        memo.set(key, total);
        return total;
      } finally {
        depth--;
      }
    };
    let cursor = 0;
    let repaired: Block[] | undefined;
    for (let index = 0; index < blocks.length; index++) {
      const first = blocks[index];
      if (bindable(first) && hop[index]) {
        const own = nextSlot(first.text, cursor);
        const tail = score(index + 1, own < limit ? own + 1 : cursor);
        let best: AlignmentScore = own < limit ? [tail[0] + 2, tail[1] + own] : tail;
        let chosen: { last: number; slot: number; text: string } | undefined;
        let combined = first.text;
        for (let f = hop[index], t = 1; f && combined.length <= maxText; f = hop[f], t++) {
          combined += blocks[f].text;
          built += blocks[f].text.length;
          if (built > 4_000_000) throw new Error("alignment budget");
          const mergeSlot = nextSlot(combined, cursor);
          if (mergeSlot < limit && (own >= limit || mergeSlot <= own)) {
            const rest = score(f + 1, mergeSlot + 1);
            const candidate: AlignmentScore = [rest[0] + t + 2, rest[1] + mergeSlot];
            if (!beats(best, candidate)) {
              best = candidate;
              chosen = { last: f, slot: mergeSlot, text: combined };
            }
          }
        }
        if (chosen) {
          repaired ??= blocks.slice(0, index);
          repaired.push(
            { ...first, text: chosen.text },
            ...blocks.slice(index + 1, chosen.last).filter(block => block.role === "system"),
          );
          cursor = chosen.slot + 1;
          index = chosen.last;
          continue;
        }
        if (own < limit) cursor = own + 1;
      } else if (bindable(first)) {
        const own = nextSlot(first.text, cursor);
        if (own < limit) cursor = own + 1;
      }
      repaired?.push(first);
    }
    return repaired ?? blocks;
  } catch {
    return mergeStatusSplitsLinear(blocks, source);
  }
}

/** Fallback when the full alignment exceeds its budget: weld only the first
 * fragment pair when its combined text is exactly the next unconsumed slot. */
function mergeStatusSplitsLinear(blocks: Block[], source: readonly OmpAssistantText[]): Block[] {
  const positions = sourcePositions(source);
  const limit = source.length;
  const nextSlot = (text: string, from: number) => slotAt(positions, text, from, limit);
  const hop = chainHops(blocks);
  let cursor = 0;
  let repaired: Block[] | undefined;
  for (let index = 0; index < blocks.length; index++) {
    const first = blocks[index];
    const end = hop[index] || undefined;
    if (
      end !== undefined && cursor < limit &&
      nextSlot(first.text + blocks[end].text, cursor) === cursor
    ) {
      repaired ??= blocks.slice(0, index);
      repaired.push({ ...first, text: first.text + blocks[end].text }, ...blocks.slice(index + 1, end));
      cursor++;
      index = end;
      continue;
    }
    if (first.role === "assistant" && !first.streaming) {
      const own = nextSlot(first.text, cursor);
      if (own < limit) cursor = own + 1;
    }
    repaired?.push(first);
  }
  return repaired ?? blocks;
}

/** Restore omitted boundaries, not turns: live interjections also stay mid-turn.
 * Exact text and one-based occurrence avoid inventing boundaries for progress
 * prose. Split coalesced blocks only when both complete source messages
 * concatenate exactly to their text; prefixes alone are not evidence.
 */
export function backfillOmpInterjections(
  blocks: Block[],
  anchors: readonly OmpInterjectionAnchor[],
  source: readonly OmpAssistantText[] = [],
): Block[] {
  const merged = mergeStatusSplits(blocks, source);
  if (anchors.length === 0) return merged;
  const positions = new Map<string, BoundaryNode[]>();
  const ids = new Map<string, BoundaryNode>();
  const nodes = merged.map((block, index): BoundaryNode => ({ block, index, offset: 0 }));
  function addPosition(node: BoundaryNode) {
    const matches = positions.get(node.block.text);
    if (matches) {
      matches.push(node);
      if (matches.length > 1 && sourceOrder(matches[matches.length - 2], node) > 0) {
        matches.sort(sourceOrder);
      }
    } else positions.set(node.block.text, [node]);
  }
  nodes.forEach((node, index) => {
    node.next = nodes[index + 1];
    ids.set(node.block.id, node);
    if (node.block.role === "assistant") addPosition(node);
  });
  const matchedLive = new Set<BoundaryNode>();
  const boundaryEnds = new Map<BoundaryNode, BoundaryNode>();
  let previous: BoundaryNode | undefined;
  // A chain's final note alone has a directly linked continuation. Its exact
  // split evidence also locates earlier notes sealing that same source message.
  const continuations = new Map<string, Map<number, string>>();
  function rememberContinuation(text: string, occurrence: number, following?: string | null) {
    if (!following) return;
    let byOccurrence = continuations.get(text);
    if (!byOccurrence) continuations.set(text, byOccurrence = new Map());
    byOccurrence.set(occurrence, following);
  }
  for (const anchor of anchors) {
    rememberContinuation(anchor.afterAssistantText, anchor.afterOccurrence, anchor.followingAssistantText);
    if (anchor.afterAssistantTextConcat !== undefined) {
      rememberContinuation(anchor.afterAssistantTextConcat, anchor.afterConcatOccurrence ?? anchor.afterOccurrence, anchor.followingAssistantTextConcat);
    }
  }
  let changed = merged !== blocks;
  for (const anchor of anchors) {
    // Keep legacy newline matches and IDs; live streams concatenate text parts.
    // Each representation has its own occurrence count, never fuzzy matching.
    function match(text: string, occurrence: number, following?: string | null) {
      following ??= continuations.get(text)?.get(occurrence);
      const exact = positions.get(text) ?? [];
      const combined = following ? positions.get(text + following) ?? [] : [];
      const candidates = combined.length ? [...exact, ...combined].sort(sourceOrder) : exact;
      return candidates[occurrence - 1];
    }
    let afterText = anchor.afterAssistantText;
    let node = match(afterText, anchor.afterOccurrence, anchor.followingAssistantText);
    if (!node && anchor.afterAssistantTextConcat !== undefined) {
      afterText = anchor.afterAssistantTextConcat;
      node = match(afterText, anchor.afterConcatOccurrence ?? anchor.afterOccurrence, anchor.followingAssistantTextConcat);
    }
    if (!node || (previous && sourceOrder(node, previous) < 0)) continue;
    previous = node;
    const id = `omp-interjection-${anchor.id}`;
    let boundary = ids.get(id);
    // Newer builds already stored live interjections with random IDs. Match
    // only the adjacent boundary, and consume each live row at most once.
    if (!boundary) {
      for (let live = node.next; live; live = live.next) {
        const block = live.block;
        if (block.role !== "system" || !block.interjection) break;
        if (
          !matchedLive.has(live) &&
          !block.id.startsWith("omp-interjection-") &&
          block.text === anchor.text &&
          block.interjection.customType === anchor.customType &&
          block.interjection.severity === (anchor.severity ?? undefined)
        ) {
          matchedLive.add(live);
          boundary = live;
          break;
        }
      }
    }
    if (!boundary) {
      const end = boundaryEnds.get(node) ?? node;
      boundary = {
        block: {
          id,
          role: "system",
          text: anchor.text,
          interjection: {
            customType: anchor.customType,
            ...(anchor.severity ? { severity: anchor.severity } : {}),
          },
        },
        index: node.index,
        offset: node.offset,
        next: end.next,
      };
      end.next = boundary;
      ids.set(id, boundary);
      changed = true;
    }
    boundaryEnds.set(node, boundary);
    if (node.block.text !== afterText) {
      const text = node.block.text;
      const split = afterText.length;
      let end = boundary;
      while (end.next?.block.role === "system" && end.next.block.interjection) {
        end = end.next;
      }
      const continuation: BoundaryNode = {
        block: {
          id: `${node.block.id}-${id}-continuation`,
          role: "assistant",
          text: text.slice(split),
        },
        index: node.index,
        offset: node.offset + split,
        next: end.next,
      };
      // The suffix belongs to this boundary, not to an insertion's old index.
      // Index it now so later source anchors can target it in this same pass.
      end.next = continuation;
      positions.set(text, positions.get(text)!.filter(match => match !== node));
      node.block = { ...node.block, text: afterText };
      addPosition(node);
      addPosition(continuation);
      ids.set(continuation.block.id, continuation);
      changed = true;
    }
  }
  if (!changed) return blocks;
  const repaired: Block[] = [];
  for (let node: BoundaryNode | undefined = nodes[0]; node; node = node.next) {
    repaired.push(node.block);
  }
  return repaired;
}
