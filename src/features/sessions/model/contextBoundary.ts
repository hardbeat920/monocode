import type { HarnessEvent } from "../../../integrations/harness/core/types";
import type { Block } from "./session";

/**
 * Keys the "Compacting context…" row for a turn, so the boundary that follows
 * replaces it instead of leaving the in-progress line behind.
 */
export const COMPACTION_STATUS_KEY = "compaction";

const COMPACTING_TEXT = "Compacting context…";
export const COMPACTED_TEXT = "Context compacted";
export const ROTATED_TEXT = "Fresh session started";

type CompactionProgress = Extract<HarnessEvent, { type: "status" }>;

/** The keyed row a harness shows while it compacts. */
export function compactingStatus(): CompactionProgress {
  return { type: "status", key: COMPACTION_STATUS_KEY, text: COMPACTING_TEXT };
}

/** Removes that row when a compaction ends without landing. */
export function compactingStatusCleared(): CompactionProgress {
  return { type: "status", key: COMPACTION_STATUS_KEY, text: "" };
}

/**
 * Blocks the agent no longer holds, judged by the latest boundary alone: an
 * earlier boundary's summary was itself compacted away. Where the harness
 * keeps something we cannot map onto blocks, nothing is claimed.
 */
export function outOfContextIds(blocks: readonly Block[]): Set<string> {
  let index = blocks.length - 1;
  while (index >= 0 && !blocks[index].contextBoundary) index--;
  const boundary = blocks[index]?.contextBoundary;
  const ids = new Set<string>();
  // A known first kept block draws the line itself; out of view, claim nothing.
  if (boundary?.keptFromBlockId) {
    const from = blocks.findIndex(
      (block) => block.id === boundary.keptFromBlockId,
    );
    for (const block of blocks.slice(0, Math.max(0, from))) ids.add(block.id);
    return ids;
  }
  const kept = boundary?.kept;
  if (kept !== "none" && kept !== "user-messages") return ids;
  for (const block of blocks.slice(0, index)) {
    if (kept === "user-messages" && block.role === "user") continue;
    ids.add(block.id);
  }
  return ids;
}

/**
 * Messages from before the latest boundary that may be gone from the agent's
 * context, so worth handing back: everything above it not known to be kept.
 * Tool rows are left out; an app-written turn or a draft was never the user's.
 */
export function resendableIds(blocks: readonly Block[]): Set<string> {
  let index = blocks.length - 1;
  while (index >= 0 && !blocks[index].contextBoundary) index--;
  const boundary = blocks[index]?.contextBoundary;
  const ids = new Set<string>();
  if (!boundary) return ids;
  const end = boundary.keptFromBlockId
    ? Math.max(
        0,
        blocks.findIndex((block) => block.id === boundary.keptFromBlockId),
      )
    : index;
  for (const block of blocks.slice(0, end)) {
    const message =
      block.role === "user"
        ? !block.internal && !block.draft && boundary.kept !== "user-messages"
        : block.role === "assistant" && !block.tool && !!block.text.trim();
    if (message) ids.add(block.id);
  }
  return ids;
}
