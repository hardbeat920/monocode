import {
  bindCursorSession,
  cancelCursorTurn,
  forgetCursorSession,
  respondCursorApproval,
  respondCursorQuestion,
  sendCursorTurn,
  steerCursorTurn,
  stopCursorSession,
} from "./cursor";
import { refreshCursorCatalog } from "./cursorCatalog";
import {
  generateCursorBranchName,
  generateCursorCommitMessage,
  generateCursorPrContent,
} from "./cursorGit";
import { generateCursorSessionTitle } from "./cursorTitle";
import {
  runCursorTextPrompt,
  stopCursorTextPrompt,
  warmupCursorText,
} from "./cursorText";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

export const cursorAdapter: HarnessAdapter = {
  id: "cursor",
  live: true,
  sendTurn: sendCursorTurn,
  steerTurn: steerCursorTurn,
  cancelTurn: cancelCursorTurn,
  respondApproval: respondCursorApproval,
  respondQuestion: respondCursorQuestion,
  stopSession: stopCursorSession,
  forgetSession: forgetCursorSession,
  bindSession: bindCursorSession,
  refreshCatalog: refreshCursorCatalog,
  generateTitle: generateCursorSessionTitle,
  generateCommitMessage: generateCursorCommitMessage,
  generatePrContent: generateCursorPrContent,
  generateBranchName: generateCursorBranchName,
  warmupText: warmupCursorText,
  runTextPrompt: runCursorTextPrompt,
  stopTextPrompt: stopCursorTextPrompt,
};

let registered = false;

export function ensureCursorRegistered(): void {
  if (registered) return;
  registerHarness(cursorAdapter);
  registered = true;
}
