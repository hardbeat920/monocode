import {
  bindCmdSession,
  cancelCmdTurn,
  forgetCmdSession,
  respondCmdApproval,
  sendCmdTurn,
  steerCmdTurn,
  stopCmdSession,
} from "./cmd";
import { refreshCmdCatalog } from "./cmdCatalog";
import {
  generateCmdBranchName,
  generateCmdCommitMessage,
  generateCmdPrContent,
} from "./cmdGit";
import { generateCmdSessionTitle } from "./cmdTitle";
import { warmupCmdText } from "./cmdText";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

export const cmdAdapter: HarnessAdapter = {
  id: "cmd",
  live: true,
  canSteer: false,
  sendTurn: sendCmdTurn,
  steerTurn: steerCmdTurn,
  cancelTurn: cancelCmdTurn,
  respondApproval: respondCmdApproval,
  stopSession: stopCmdSession,
  forgetSession: forgetCmdSession,
  bindSession: bindCmdSession,
  refreshCatalog: refreshCmdCatalog,
  generateTitle: generateCmdSessionTitle,
  generateCommitMessage: generateCmdCommitMessage,
  generatePrContent: generateCmdPrContent,
  generateBranchName: generateCmdBranchName,
  warmupText: warmupCmdText,
};

let registered = false;

export function ensureCmdRegistered(): void {
  if (registered) return;
  registerHarness(cmdAdapter);
  registered = true;
}
