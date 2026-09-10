import type { OmpInterjectionAnchor } from "./fs";
import type { Block } from "./session";

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
  const positions = new Map<string, number[]>();
  const ids = new Map<string, number>();
  blocks.forEach((block, index) => {
    ids.set(block.id, index);
    if (block.role !== "assistant") return;
    const matches = positions.get(block.text);
    if (matches) matches.push(index);
    else positions.set(block.text, [index]);
  });
  const insertions = new Map<number, Block[]>();
  const matchedLive = new Set<number>();
  const splitOffsets = new Map<number, number>();
  const boundaryEnds = new Map<number, number>();
  let previous = -1;
  for (const anchor of anchors) {
    const exact = positions.get(anchor.afterAssistantText) ?? [];
    const combined = anchor.followingAssistantText
      ? positions.get(anchor.afterAssistantText + anchor.followingAssistantText) ?? []
      : [];
    const candidates = combined.length
      ? [...exact, ...combined].sort((a, b) => a - b)
      : exact;
    const index = candidates[anchor.afterOccurrence - 1];
    if (index == null || index < previous) continue;
    previous = index;
    const id = `omp-interjection-${anchor.id}`;
    const existing = ids.get(id);
    if (existing != null) {
      boundaryEnds.set(index, existing);
      continue;
    }
    // Newer builds already stored live interjections with random IDs. Match
    // only the adjacent boundary, and consume each live row at most once.
    let liveIndex = index + 1;
    for (; liveIndex < blocks.length; liveIndex += 1) {
      const live = blocks[liveIndex];
      if (live.role !== "system" || !live.interjection) break;
      if (
        !matchedLive.has(liveIndex) &&
        !live.id.startsWith("omp-interjection-") &&
        live.text === anchor.text &&
        live.interjection.customType === anchor.customType &&
        live.interjection.severity === (anchor.severity ?? undefined)
      ) {
        matchedLive.add(liveIndex);
        break;
      }
    }
    if (matchedLive.has(liveIndex)) {
      boundaryEnds.set(index, liveIndex);
      continue;
    }
    const block: Block = {
      id,
      role: "system",
      text: anchor.text,
      interjection: {
        customType: anchor.customType,
        ...(anchor.severity ? { severity: anchor.severity } : {}),
      },
    };
    const insertionIndex = boundaryEnds.get(index) ?? index;
    const pending = insertions.get(insertionIndex);
    if (pending) pending.push(block);
    else insertions.set(insertionIndex, [block]);
    ids.set(id, insertionIndex);
    if (blocks[index].text !== anchor.afterAssistantText) {
      splitOffsets.set(index, anchor.afterAssistantText.length);
    }
  }
  if (insertions.size === 0) return blocks;
  const repaired: Block[] = [];
  blocks.forEach((block, index) => {
    const split = splitOffsets.get(index);
    repaired.push(split == null ? block : { ...block, text: block.text.slice(0, split) });
    const pending = insertions.get(index);
    if (pending) repaired.push(...pending);
    if (split != null && pending) {
      repaired.push({
        id: `${pending[pending.length - 1].id}-continuation`,
        role: "assistant",
        text: block.text.slice(split),
      });
    }
  });
  return repaired;
}
