import {
  bindOpenCodeSession,
  cancelOpenCodeTurn,
  compactOpenCodeContext,
  forgetOpenCodeSession,
  respondOpenCodeApproval,
  respondOpenCodeQuestion,
  sendOpenCodeTurn,
  steerOpenCodeTurn,
  stopOpenCodeSession,
} from "./opencode";
import { refreshOpenCodeCatalog } from "./opencodeCatalog";
import {
  generateOpenCodeBranchName,
  generateOpenCodeCommitMessage,
  generateOpenCodePrContent,
} from "./opencodeGit";
import { generateOpenCodeSessionTitle } from "./opencodeTitle";
import { warmupOpenCodeText } from "./opencodeText";
import { registerHarness, type HarnessAdapter } from "./registry";
import type { HarnessId } from "../session";

export const openCodeAdapter: HarnessAdapter = {
  id: "opencode",
  live: true,
  sendTurn: sendOpenCodeTurn,
  compactContext: compactOpenCodeContext,
  steerTurn: steerOpenCodeTurn,
  cancelTurn: cancelOpenCodeTurn,
  respondApproval: respondOpenCodeApproval,
  respondQuestion: respondOpenCodeQuestion,
  stopSession: stopOpenCodeSession,
  forgetSession: forgetOpenCodeSession,
  bindSession: bindOpenCodeSession,
  refreshCatalog: refreshOpenCodeCatalog,
  generateTitle: generateOpenCodeSessionTitle,
  generateCommitMessage: generateOpenCodeCommitMessage,
  generatePrContent: generateOpenCodePrContent,
  generateBranchName: generateOpenCodeBranchName,
  warmupText: warmupOpenCodeText,
};

let registered = false;

export function ensureOpenCodeRegistered(): void {
  if (registered) return;
  registerHarness(openCodeAdapter);
  registered = true;
}

/**
 * OpenAI-compatible providers use the OpenCode runtime, but remain separate
 * provider tabs in MonoCode. The selected model's native id tells OpenCode
 * which provider/model pair to route to.
 */
export function ensureOpenCodeProviderRegistered(id: Exclude<HarnessId, "opencode">): void {
  if (registeredProviders.has(id)) return;
  registerHarness({ ...openCodeAdapter, id, refreshCatalog: undefined });
  registeredProviders.add(id);
}

const registeredProviders = new Set<HarnessId>();
