import type { OmpInterjectionAnchor } from "./fs";
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

/** Restore omitted boundaries, not turns: live interjections also stay mid-turn.
 * Exact text and one-based occurrence avoid inventing boundaries for progress
 * prose. Split coalesced blocks only when both complete source messages
 * concatenate exactly to their text; prefixes alone are not evidence.
 */
export function backfillOmpInterjections(
  blocks: Block[],
  anchors: readonly OmpInterjectionAnchor[],
): Block[] {
  if (anchors.length === 0) return blocks;
  const positions = new Map<string, BoundaryNode[]>();
  const ids = new Map<string, BoundaryNode>();
  const nodes = blocks.map((block, index): BoundaryNode => ({ block, index, offset: 0 }));
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
  let changed = false;
  for (const anchor of anchors) {
    const exact = positions.get(anchor.afterAssistantText) ?? [];
    const combined = anchor.followingAssistantText
      ? positions.get(anchor.afterAssistantText + anchor.followingAssistantText) ?? []
      : [];
    const candidates = combined.length
      ? [...exact, ...combined].sort(sourceOrder)
      : exact;
    const node = candidates[anchor.afterOccurrence - 1];
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
    if (node.block.text !== anchor.afterAssistantText) {
      const text = node.block.text;
      const split = anchor.afterAssistantText.length;
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
      node.block = { ...node.block, text: anchor.afterAssistantText };
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
