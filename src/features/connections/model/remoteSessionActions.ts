import type {
  Attachment,
  ComposerTurnOptions,
  HarnessId,
  PlanBuildTarget,
  RuntimeMode,
} from "../../sessions/model/session";
import type { ApprovalDecision, UserQuestionReply } from "../../../integrations/harness";

export type RemoteSessionChoice = {
  harness?: HarnessId;
  model?: string;
  modelSettings?: Record<string, string>;
  runtimeMode?: RuntimeMode;
};

type RemoteActions = {
  buildPlan: (blockId: string, target?: PlanBuildTarget) => void;
  submit: (text: string, attachments: Attachment[], options?: ComposerTurnOptions) => boolean | void;
  saveDraft: (text: string, attachments: Attachment[]) => boolean | void;
  stop: () => void;
  compact: () => boolean;
  configure: (choice: RemoteSessionChoice) => Promise<void>;
  approve: (requestId: number, decision: ApprovalDecision) => void;
  answer: (requestId: number, reply: UserQuestionReply) => void;
};

const actions = new Map<string, RemoteActions>();

export function registerRemoteSessionActions(shellId: string, value: RemoteActions) {
  actions.set(shellId, value);
  return () => {
    if (actions.get(shellId) === value) actions.delete(shellId);
  };
}

export function buildRemotePlan(shellId: string, blockId: string, target?: PlanBuildTarget) {
  actions.get(shellId)?.buildPlan(blockId, target);
}

export function remoteSessionActions(shellId: string): RemoteActions | undefined {
  return actions.get(shellId);
}
