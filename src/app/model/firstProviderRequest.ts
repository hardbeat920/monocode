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
  operatorCli?: string;
};

/** Compose the exact first request before exporting history or changing providers. */
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
  let text = input.orchestratorPrompt(
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
  if (input.operatorCli) {
    const cli = input.operatorCli;
    text += `\n\n<monocode_app>\nThe user's Operator command enables app access in this thread, including later turns without the command. You can start session tabs or split session panes right or down, list and create project worktrees, choose a new session's checkout, read and continue other project sessions, save unsent drafts, organize session folders, and read or write saved notes through its local CLI. Run \`${cli} --help\` for exact commands and JSON fields, then use it as needed for the user's request. When reading another session, start with its latest two or three user/assistant exchanges. Request older exchanges with nextBefore or a larger excerpt only if needed. The CLI uses a session credential already in your environment; never print it. New sessions inherit this session's permission mode unless runtimeMode is set explicitly. For a new session with a draft, call sessions.start with its prompt and draft:true; do not submit a seed prompt. The returned ID can be used as besideSessionId to split its pane again or moved into a folder immediately. A normal sessions.start submits its prompt but returns after acceptance, so do not wait for that agent to finish before organizing it.\n</monocode_app>`;
  }
  return text;
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
