import {
  bindCodexSession,
  cancelCodexTurn,
  compactCodexContext,
  forgetCodexSession,
  respondCodexApproval,
  sendCodexTurn,
  steerCodexTurn,
  stopCodexSession,
} from "./codex";
import {
  generateCodexBranchName,
  generateCodexCommitMessage,
  generateCodexPrContent,
} from "./codexGit";
import { refreshCodexCatalog } from "./codexCatalog";
import { generateCodexSessionTitle } from "./codexTitle";
import { warmupCodexText } from "./codexText";
import { registerHarness, type HarnessAdapter } from "./registry";

export const codexAdapter: HarnessAdapter = {
  id: "codex",
  live: true,
  sendTurn: sendCodexTurn,
  compactContext: compactCodexContext,
  steerTurn: steerCodexTurn,
  cancelTurn: cancelCodexTurn,
  respondApproval: respondCodexApproval,
  stopSession: stopCodexSession,
  forgetSession: forgetCodexSession,
  bindSession: bindCodexSession,
  refreshCatalog: refreshCodexCatalog,
  generateTitle: generateCodexSessionTitle,
  generateCommitMessage: generateCodexCommitMessage,
  generatePrContent: generateCodexPrContent,
  generateBranchName: generateCodexBranchName,
  warmupText: warmupCodexText,
};

let registered = false;

export function ensureCodexRegistered(): void {
  if (registered) return;
  registerHarness(codexAdapter);
  registered = true;
}
