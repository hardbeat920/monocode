import { createContext, useContext, useEffect, useState } from "react";
import { loadToolDetail } from "../data/sessionStore";
import type { Block } from "../model/session";

/** The saved session that rows in this transcript fetch omitted output from. */
export const ToolDetailSession = createContext<string | undefined>(undefined);

const OUTPUT_UNAVAILABLE = "Output is no longer available.";

/** Synthetic subagent-step rows read their output from the parent block. */
const stepOwners = new WeakMap<Block, { blockId: string; stepId: string }>();

export function ownStepBlock(block: Block, blockId: string, stepId: string) {
  stepOwners.set(block, { blockId, stepId });
  return block;
}

export type ToolDetail = {
  /** Inline or fetched output; undefined while a fetch is pending. */
  detail?: string;
  /** Output exists in storage, so the row can expand before it is fetched. */
  omitted: boolean;
  pending: boolean;
};

/**
 * A tool row's output. Loaded transcripts leave large output in storage
 * (`detailOmitted`), and the row fetches it only once it is opened.
 */
export function useToolDetail(block: Block, open: boolean): ToolDetail {
  const sessionId = useContext(ToolDetailSession);
  const inline = block.tool?.detail;
  const omitted = !inline && !!block.tool?.detailOmitted && !!sessionId;
  const owner = stepOwners.get(block);
  const blockId = owner?.blockId ?? block.id;
  const stepId = owner?.stepId;
  const key = `${sessionId}\0${blockId}\0${stepId ?? ""}`;
  const [loaded, setLoaded] = useState<{ key: string; detail: string }>();
  const fetched = loaded?.key === key ? loaded.detail : undefined;
  const fetching = omitted && open && fetched === undefined;
  useEffect(() => {
    if (!fetching || !sessionId) return;
    let cancelled = false;
    const settle = (detail: string | null) => {
      if (!cancelled) setLoaded({ key, detail: detail ?? OUTPUT_UNAVAILABLE });
    };
    loadToolDetail(sessionId, blockId, stepId).then(settle, () => settle(null));
    return () => {
      cancelled = true;
    };
  }, [fetching, sessionId, blockId, stepId, key]);
  if (!omitted) return { detail: inline, omitted: false, pending: false };
  return { detail: fetched, omitted: true, pending: fetching };
}
