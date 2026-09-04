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
