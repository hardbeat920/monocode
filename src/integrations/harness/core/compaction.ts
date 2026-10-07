import {
  compactingStatus,
  compactingStatusCleared,
} from "../../../features/sessions/model/contextBoundary";
import type { CompactContextInput, HarnessEvent } from "./types";

/**
 * Run a compaction the user asked for, the same way locally and on a remote
 * host. A "Compacting context…" row shows while it runs and the harness's own
 * boundary replaces it, marked manual: not every harness says who started a
 * compaction, but nothing else runs on the session meanwhile. A cancelled
 * compaction resolves quietly and a failed one rejects; neither compacted
 * anything, so the row is cleared instead of standing in for a boundary.
 */
export async function runManualCompaction(
  input: CompactContextInput,
  compact: (input: CompactContextInput) => Promise<void>,
): Promise<void> {
  let marked = false;
  const onEvent = (event: HarnessEvent) => {
    if (event.type !== "context.compacted") return input.onEvent(event);
    marked = true;
    input.onEvent({ ...event, trigger: "manual" });
  };
  const clear = () => {
    if (!marked) input.onEvent(compactingStatusCleared());
  };
  input.onEvent(compactingStatus());
  try {
    await compact({ ...input, onEvent });
  } catch (error) {
    clear();
    throw error;
  }
  clear();
}
