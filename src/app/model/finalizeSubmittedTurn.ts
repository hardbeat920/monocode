import type {
  ComposerTurnOptions,
  Session,
} from "../../features/sessions/model/session";
import {
  completeOrchestrationProposal,
  withOrchestrationProposal,
  type OrchestrationProposal,
} from "../../features/orchestration/model/orchestrationPlan";
import { isProviderFailureText } from "../../features/sessions/model/plan";
import {
  promoteLastAssistantToPlan,
  stopStreaming,
} from "../../integrations/harness/core/apply";

type SubmittedPlanOptions = {
  intent: ComposerTurnOptions["intent"];
  planEventKey: string;
  nativePlanSeen: boolean;
  providerFailureSeen: boolean;
  buildSucceeded: boolean;
  approvedPlanId?: string;
  proposalId?: string;
  proposalDraft?: OrchestrationProposal;
  completedProposal?: OrchestrationProposal;
  proposalText: string;
  error?: string;
};

/** Finalize only the submitted run's plans; a later provider run owns live state. */
export function finalizeSubmittedTurn(
  session: Session,
  submitted: Session,
  options: SubmittedPlanOptions,
): Session {
  const {
    intent,
    planEventKey,
    nativePlanSeen,
    providerFailureSeen,
    buildSucceeded,
    approvedPlanId,
    proposalId,
    proposalDraft,
    completedProposal,
    proposalText,
    error,
  } = options;
  const stopped = stopStreaming(submitted);
  const providerFailed =
    providerFailureSeen ||
    isProviderFailureText(lastAssistantTextInTurn(stopped));
  let finalized =
    proposalDraft && proposalId
      ? withOrchestrationProposal(
          stopped,
          proposalId,
          completedProposal && !providerFailed && buildSucceeded
            ? completedProposal
            : completeOrchestrationProposal(
                proposalDraft,
                proposalText,
                providerFailed || !buildSucceeded
                  ? (error ?? "The lead could not finish planning.")
                  : undefined,
              ),
        )
      : intent === "plan" && !nativePlanSeen && !providerFailed
        ? promoteLastAssistantToPlan(stopped, planEventKey)
        : stopped;
  if (approvedPlanId && intent === "build") {
    finalized = {
      ...finalized,
      blocks: finalized.blocks.map((block) =>
        block.id === approvedPlanId && block.role === "plan"
          ? {
              ...block,
              plan: {
                ...(block.plan ?? { status: "ready" }),
                status: buildSucceeded && !providerFailed ? "built" : "ready",
              },
            }
          : block,
      ),
    };
  }
  const submittedBlocks = new Map(
    submitted.blocks.map((block) => [block.id, block]),
  );
  const plans = new Map(
    finalized.blocks
      .filter(
        (block) =>
          block.role === "plan" && block !== submittedBlocks.get(block.id),
      )
      .map((block) => [block.id, block]),
  );
  const current =
    session.providerActive && !providerFailureSeen
      ? session
      : stopStreaming(session);
  return {
    ...current,
    blocks: current.blocks.map((block) => plans.get(block.id) ?? block),
  };
}

/** Ignore queued user blocks when finding the submitted run's final answer. */
function lastAssistantTextInTurn(session: Session): string {
  for (let index = session.blocks.length - 1; index >= 0; index -= 1) {
    const block = session.blocks[index];
    if (session.queuedMessages?.some((message) => message.blockId === block.id))
      continue;
    if (block.role === "user") return "";
    if (block.role === "assistant" && block.text.trim()) return block.text;
  }
  return "";
}
