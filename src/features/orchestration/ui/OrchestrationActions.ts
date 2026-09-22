import { createContext } from "react";
import type { OrchestrationProposal } from "../model/orchestrationPlan";
import type { HarnessId } from "../../sessions/model/session";

export type OrchestrationWorkerDetail = {
  sessionId: string;
  leadId: string;
  title: string;
  harness: HarnessId;
};

/**
 * The lead's sidebar card lists workers. Approvals still go to the lead, not
 * to the user; `openDetails` is the one way to watch a worker's transcript.
 */
export const OrchestrationWorkers = createContext<{
  selectedId: string | null;
  inspect(sessionId: string | null): void;
  /**
   * Open this worker beside its lead, for when the card's model line is not
   * enough. Absent wherever the card renders without a workspace behind it.
   */
  openDetails?(worker: OrchestrationWorkerDetail): void;
}>({ selectedId: null, inspect: () => {} });

// Shared by transcript cards in both ordinary and split session panes.
export const OrchestrationActions = createContext<{
  update(
    leadId: string,
    blockId: string,
    proposal: OrchestrationProposal,
  ): void;
  confirm(leadId: string, blockId: string): Promise<void>;
  retry(leadId: string, blockId: string): void;
  open(sessionId: string): void;
  /** Open every worker of a run as tabs beside the lead, not sidebar rows. */
  openAgents?(workers: OrchestrationWorkerDetail[]): void;
} | null>(null);
