import {
  COMPACTING_TEXT,
  COMPACTION_STATUS_KEY,
} from "../../../features/sessions/model/contextBoundary";
import type { CompactContextInput, HarnessEvent } from "./types";

/**
 * Run a manual compaction with the same transcript chrome locally and on a
 * remote host: a "Compacting context…" row while it runs, which the harness's
 * own boundary replaces. A cancelled compaction resolves quietly and a failed
 * one rejects; neither compacted anything, so the row is cleared instead of
 * standing in for a boundary that never happened.
 */
export async function compactWithProgress(
  input: CompactContextInput,
  compact: (input: CompactContextInput) => Promise<void>,
): Promise<void> {
  let marked = false;
  const onEvent = (event: HarnessEvent) => {
    if (event.type === "context.compacted") marked = true;
    input.onEvent(event);
  };
  const clear = () => {
    if (!marked) {
      input.onEvent({ type: "status", key: COMPACTION_STATUS_KEY, text: "" });
    }
  };
  input.onEvent({
    type: "status",
    key: COMPACTION_STATUS_KEY,
    text: COMPACTING_TEXT,
  });
  try {
    await compact({ ...input, onEvent });
  } catch (error) {
    clear();
    throw error;
  }
  clear();
}
