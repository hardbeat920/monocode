import type {
  Attachment,
  ComposerTurnOptions,
  PlanBuildTarget,
} from "../../sessions/model/session";
import type {
  ApprovalDecision,
  UserQuestionReply,
} from "../../../integrations/harness";

type RemoteActions = {
  buildPlan: (
    blockId: string,
    target?: PlanBuildTarget,
  ) => boolean | Promise<boolean>;
  submit: (
    text: string,
    attachments: Attachment[],
    options?: ComposerTurnOptions,
  ) => boolean | void;
  saveDraft: (text: string, attachments: Attachment[]) => boolean | void;
  stop: () => void;
  compact: () => boolean;
  approve: (requestId: number, decision: ApprovalDecision) => void;
  answer: (requestId: number, reply: UserQuestionReply) => void;
};

const actions = new Map<string, RemoteActions>();

export function registerRemoteSessionActions(
  shellId: string,
  value: RemoteActions,
) {
  actions.set(shellId, value);
  return () => {
    if (actions.get(shellId) === value) actions.delete(shellId);
  };
}

export function buildRemotePlan(
  shellId: string,
  blockId: string,
  target?: PlanBuildTarget,
): boolean | Promise<boolean> | undefined {
  return actions.get(shellId)?.buildPlan(blockId, target);
}

export function remoteSessionActions(
  shellId: string,
): RemoteActions | undefined {
  return actions.get(shellId);
}
