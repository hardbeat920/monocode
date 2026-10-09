import {
  inboxAskPrompt,
  type InboxAskContext,
} from "../../features/inbox/model/inboxAsk";
import {
  orchestrationPlanningPrompt,
  orchestrationRepairPrompt,
  type OrchestrationProposal,
} from "../../features/orchestration/model/orchestrationPlan";
import { wrapHandoffPrompt } from "../../features/sessions/model/handoff";
import { CONTINUE_PROMPT } from "../../features/sessions/model/inFlight";
import {
  buildPlanPrompt,
  planTurnPrompt,
} from "../../features/sessions/model/plan";
import {
  currentAttachmentTokens,
  type PortableContextOptions,
} from "../../features/sessions/model/portableContext";
import type {
  Attachment,
  HarnessId,
  TurnIntent,
} from "../../features/sessions/model/session";

type FirstProviderRequestInput = {
  prompt: string;
  intent: TurnIntent;
  approvedPlan?: string;
  proposal?: OrchestrationProposal;
  orchestrationRetry?: Pick<OrchestrationProposal, "error" | "response">;
  rawCommand?: boolean;
  handoff?: { from: HarnessId; text: string } | null;
  earlierRequests?: string[];
  inboxAsk?: InboxAskContext;
  orchestratorPrompt: (text: string) => string;
};

/** Compose the request text before exporting history or changing providers. The caller appends app context. */
export function firstProviderRequest(input: FirstProviderRequestInput): string {
  const prompt =
    input.intent === "build" && input.approvedPlan != null
      ? buildPlanPrompt(input.approvedPlan)
      : input.prompt;
  const turnPrompt = input.proposal
    ? input.orchestrationRetry?.response
      ? orchestrationRepairPrompt({
          ...input.proposal,
          error: input.orchestrationRetry.error,
          response: input.orchestrationRetry.response,
        })
      : orchestrationPlanningPrompt(
          prompt,
          input.proposal.settings,
          input.proposal.checkoutCwd ?? input.proposal.cwd,
        )
    : input.intent === "plan" && !input.rawCommand
      ? planTurnPrompt(prompt)
      : prompt;
  return input.orchestratorPrompt(
    inboxAskPrompt(
      input.rawCommand ? undefined : input.inboxAsk,
      input.handoff && !input.rawCommand
        ? wrapHandoffPrompt(
            input.handoff.text,
            input.handoff.from,
            turnPrompt.trim() || CONTINUE_PROMPT,
            input.earlierRequests,
          )
        : turnPrompt,
    ),
  );
}

export function firstProviderRequestBudget(request: {
  text: string;
  attachments: Attachment[];
}): Pick<PortableContextOptions, "currentRequest" | "attachmentTokens"> {
  return {
    currentRequest: request.text,
    attachmentTokens: currentAttachmentTokens(request.attachments),
  };
}
