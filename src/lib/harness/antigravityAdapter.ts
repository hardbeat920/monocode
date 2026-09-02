import {
  bindAntigravitySession,
  cancelAntigravityTurn,
  forgetAntigravitySession,
  respondAntigravityApproval,
  respondAntigravityQuestion,
  sendAntigravityTurn,
  steerAntigravityTurn,
  stopAntigravitySession,
} from "./antigravity";
import { refreshAntigravityCatalog } from "./antigravityCatalog";
import {
  generateAntigravityBranchName,
  generateAntigravityCommitMessage,
  generateAntigravityPrContent,
} from "./antigravityGit";
import { generateAntigravitySessionTitle } from "./antigravityTitle";
import { warmupAntigravityText } from "./antigravityText";
import { registerHarness, type HarnessAdapter } from "./registry";

export const antigravityAdapter: HarnessAdapter = {
  id: "antigravity",
  live: true,
  canSteer: true,
  sendTurn: sendAntigravityTurn,
  steerTurn: steerAntigravityTurn,
  cancelTurn: cancelAntigravityTurn,
  respondApproval: respondAntigravityApproval,
  respondQuestion: respondAntigravityQuestion,
  stopSession: stopAntigravitySession,
  forgetSession: forgetAntigravitySession,
  bindSession: bindAntigravitySession,
  refreshCatalog: refreshAntigravityCatalog,
  generateTitle: generateAntigravitySessionTitle,
  generateCommitMessage: generateAntigravityCommitMessage,
  generatePrContent: generateAntigravityPrContent,
  generateBranchName: generateAntigravityBranchName,
  warmupText: warmupAntigravityText,
};

let registered = false;

export function ensureAntigravityRegistered(): void {
  if (registered) return;
  registerHarness(antigravityAdapter);
  registered = true;
}
