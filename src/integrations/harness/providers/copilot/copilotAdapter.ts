import {
  bindCopilotSession,
  cancelCopilotTurn,
  forgetCopilotSession,
  respondCopilotApproval,
  sendCopilotTurn,
  steerCopilotTurn,
  stopCopilotSession,
} from "./copilot";
import { refreshCopilotCatalog } from "./copilotCatalog";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

export const copilotAdapter: HarnessAdapter = {
  id: "copilot",
  live: true,
  sendTurn: sendCopilotTurn,
  steerTurn: steerCopilotTurn,
  cancelTurn: cancelCopilotTurn,
  respondApproval: respondCopilotApproval,
  stopSession: stopCopilotSession,
  forgetSession: forgetCopilotSession,
  bindSession: bindCopilotSession,
  refreshCatalog: refreshCopilotCatalog,
};

let registered = false;

export function ensureCopilotRegistered(): void {
  if (registered) return;
  registerHarness(copilotAdapter);
  registered = true;
}
