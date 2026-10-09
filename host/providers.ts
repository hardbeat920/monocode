import * as codex from "../src/integrations/harness/providers/codex/codex";
import * as claude from "../src/integrations/harness/providers/claude/claude";
import * as cursor from "../src/integrations/harness/providers/cursor/cursor";
import * as grok from "../src/integrations/harness/providers/grok/grok";
import * as opencode from "../src/integrations/harness/providers/opencode/opencode";
import * as pi from "../src/integrations/harness/providers/pi/pi";
import * as omp from "../src/integrations/harness/providers/omp/omp";
import * as fx from "../src/integrations/harness/providers/fx/fx";
import * as copilot from "../src/integrations/harness/providers/copilot/copilot";
import * as hermes from "../src/integrations/harness/providers/hermes/hermes";
import * as antigravity from "../src/integrations/harness/providers/antigravity/antigravity";
import type {
  SendTurnInput,
  CompactContextInput,
  ApprovalDecision,
} from "../src/integrations/harness/core/types";
import type { UserQuestionReply } from "../src/features/sessions/model/userQuestion";
import type { RemoteProvider } from "../src/features/connections/model/protocol";
import type { GeneratedSessionTitle } from "../src/features/sessions/model/sessionTitle";
import { generateCodexSessionTitle } from "../src/integrations/harness/providers/codex/codexTitle";
import { generateClaudeSessionTitle } from "../src/integrations/harness/providers/claude/claudeTitle";
import { generateCodexBranchName } from "../src/integrations/harness/providers/codex/codexGit";
import { generateClaudeBranchName } from "../src/integrations/harness/providers/claude/claudeGit";
import { generateCursorSessionTitle } from "../src/integrations/harness/providers/cursor/cursorTitle";
import { generateGrokSessionTitle } from "../src/integrations/harness/providers/grok/grokTitle";
import { generateOpenCodeSessionTitle } from "../src/integrations/harness/providers/opencode/opencodeTitle";
import {
  generatePiSessionTitle,
  generateOmpSessionTitle,
} from "../src/integrations/harness/providers/pi/piTitle";
import {
  PI_FLAVOR,
  OMP_FLAVOR,
} from "../src/integrations/harness/providers/pi/piFlavor";
import { respondQuestion as respondPiQuestion } from "../src/integrations/harness/providers/pi/piFamily";

export interface HostProvider {
  send(input: SendTurnInput): Promise<void>;
  compact?(input: CompactContextInput): Promise<void>;
  cancel(id: string): Promise<void>;
  stop(id: string): Promise<void>;
  bind(id: string, providerId: string, cwd: string): void;
  approve(id: string, request: number, decision: ApprovalDecision): void;
  answer(id: string, request: number, reply: UserQuestionReply): void;
  generateTitle?(input: {
    sessionId: string;
    cwd: string;
    message: string;
  }): Promise<GeneratedSessionTitle | null>;
  generateBranchName?(cwd: string, message: string): Promise<string | null>;
}

export const hostProviders: Record<RemoteProvider, HostProvider> = {
  codex: {
    send: codex.sendCodexTurn,
    compact: codex.compactCodexContext,
    cancel: codex.cancelCodexTurn,
    stop: codex.forgetCodexSession,
    bind: codex.bindCodexSession,
    approve: codex.respondCodexApproval,
    answer: codex.respondCodexQuestion,
    generateTitle: generateCodexSessionTitle,
    generateBranchName: generateCodexBranchName,
  },
  claude: {
    send: claude.sendClaudeTurn,
    compact: claude.compactClaudeContext,
    cancel: claude.cancelClaudeTurn,
    stop: claude.forgetClaudeSession,
    bind: claude.bindClaudeSession,
    approve: claude.respondClaudeApproval,
    answer: claude.respondClaudeQuestion,
    generateTitle: generateClaudeSessionTitle,
    generateBranchName: generateClaudeBranchName,
  },
  cursor: {
    send: cursor.sendCursorTurn,
    cancel: cursor.cancelCursorTurn,
    stop: cursor.forgetCursorSession,
    bind: cursor.bindCursorSession,
    approve: cursor.respondCursorApproval,
    answer: cursor.respondCursorQuestion,
    generateTitle: generateCursorSessionTitle,
  },
  grok: {
    send: grok.sendGrokTurn,
    compact: grok.compactGrokContext,
    cancel: grok.cancelGrokTurn,
    stop: grok.forgetGrokSession,
    bind: grok.bindGrokSession,
    approve: grok.respondGrokApproval,
    answer: grok.respondGrokQuestion,
    generateTitle: generateGrokSessionTitle,
  },
  opencode: {
    send: opencode.sendOpenCodeTurn,
    compact: opencode.compactOpenCodeContext,
    cancel: opencode.cancelOpenCodeTurn,
    stop: opencode.forgetOpenCodeSession,
    bind: opencode.bindOpenCodeSession,
    approve: opencode.respondOpenCodeApproval,
    answer: opencode.respondOpenCodeQuestion,
    generateTitle: generateOpenCodeSessionTitle,
  },
  pi: {
    send: pi.sendPiTurn,
    compact: pi.compactPiContext,
    cancel: pi.cancelPiTurn,
    stop: pi.forgetPiSession,
    bind: pi.bindPiSession,
    approve: pi.respondPiApproval,
    answer: (id, request, reply) =>
      respondPiQuestion(PI_FLAVOR, id, request, reply),
    generateTitle: generatePiSessionTitle,
  },
  omp: {
    send: omp.sendOmpTurn,
    compact: omp.compactOmpContext,
    cancel: omp.cancelOmpTurn,
    stop: omp.forgetOmpSession,
    bind: omp.bindOmpSession,
    approve: omp.respondOmpApproval,
    answer: (id, request, reply) =>
      respondPiQuestion(OMP_FLAVOR, id, request, reply),
    generateTitle: generateOmpSessionTitle,
  },
  fx: {
    send: fx.sendFxTurn,
    cancel: fx.cancelFxTurn,
    stop: fx.forgetFxSession,
    bind: fx.bindFxSession,
    approve: fx.respondFxApproval,
    answer: unsupportedQuestion,
  },
  copilot: {
    send: copilot.sendCopilotTurn,
    compact: copilot.compactCopilotContext,
    cancel: copilot.cancelCopilotTurn,
    stop: copilot.forgetCopilotSession,
    bind: copilot.bindCopilotSession,
    approve: copilot.respondCopilotApproval,
    answer: unsupportedQuestion,
  },
  hermes: {
    send: hermes.sendHermesTurn,
    cancel: hermes.cancelHermesTurn,
    stop: hermes.forgetHermesSession,
    bind: hermes.bindHermesSession,
    approve: hermes.respondHermesApproval,
    answer: unsupportedQuestion,
  },
  antigravity: {
    send: antigravity.sendAntigravityTurn,
    cancel: antigravity.cancelAntigravityTurn,
    stop: antigravity.forgetAntigravitySession,
    bind: antigravity.bindAntigravitySession,
    approve: antigravity.respondAntigravityApproval,
    answer: unsupportedQuestion,
  },
};

function unsupportedQuestion(): never {
  throw new Error("This provider does not support questions");
}
