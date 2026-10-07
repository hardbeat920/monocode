import type { Block } from "./session";

/**
 * Keys the "Compacting context…" row for a turn, so the boundary that follows
 * replaces it instead of leaving the in-progress line behind.
 */
export const COMPACTION_STATUS_KEY = "compaction";

export const COMPACTING_TEXT = "Compacting context…";
export const COMPACTED_TEXT = "Context compacted";

/**
 * Blocks the agent no longer holds, judged by the latest boundary alone: an
 * earlier boundary's summary was itself compacted away. Where the harness
 * keeps something we cannot map onto blocks, nothing is claimed.
 */
export function outOfContextIds(blocks: readonly Block[]): Set<string> {
  let index = blocks.length - 1;
  while (index >= 0 && !blocks[index].contextBoundary) index--;
  const kept = blocks[index]?.contextBoundary?.kept;
  const ids = new Set<string>();
  if (kept !== "none" && kept !== "user-messages") return ids;
  for (const block of blocks.slice(0, index)) {
    if (kept === "user-messages" && block.role === "user") continue;
    ids.add(block.id);
  }
  return ids;
}
