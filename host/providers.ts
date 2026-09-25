import * as codex from "../src/integrations/harness/providers/codex/codex";
import * as claude from "../src/integrations/harness/providers/claude/claude";
import type {
  SendTurnInput,
  ApprovalDecision,
} from "../src/integrations/harness/core/types";
import type { UserQuestionReply } from "../src/features/sessions/model/userQuestion";
import type { RemoteProvider } from "../src/features/connections/model/protocol";

export interface HostProvider {
  send(input: SendTurnInput): Promise<void>;
  cancel(id: string): Promise<void>;
  stop(id: string): Promise<void>;
  bind(id: string, providerId: string, cwd: string): void;
  approve(id: string, request: number, decision: ApprovalDecision): void;
  answer(id: string, request: number, reply: UserQuestionReply): void;
}

export const hostProviders: Record<RemoteProvider, HostProvider> = {
  codex: {
    send: codex.sendCodexTurn,
    cancel: codex.cancelCodexTurn,
    stop: codex.forgetCodexSession,
    bind: codex.bindCodexSession,
    approve: codex.respondCodexApproval,
    answer: codex.respondCodexQuestion,
  },
  claude: {
    send: claude.sendClaudeTurn,
    cancel: claude.cancelClaudeTurn,
    stop: claude.forgetClaudeSession,
    bind: claude.bindClaudeSession,
    approve: claude.respondClaudeApproval,
    answer: claude.respondClaudeQuestion,
  },
};
