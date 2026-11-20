import {
  bindCopilotSession,
  cancelCopilotTurn,
  compactCopilotContext,
  forgetCopilotSession,
  respondCopilotApproval,
  sendCopilotTurn,
  steerCopilotTurn,
  stopCopilotSession,
  copilotCommands,
  subscribeCopilotCommands,
} from "./copilot";
import {
  discoverCopilotCommands,
  refreshCopilotCatalog,
} from "./copilotCatalog";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

export const copilotAdapter: HarnessAdapter = {
  id: "copilot",
  live: true,
  canSteer: false,
  commands: {
    rawSlashCommands: true,
    discover: async ({ cwd, sessionId }) => {
      const commands = copilotCommands(sessionId);
      return commands.length > 0 ? commands : discoverCopilotCommands(cwd);
    },
    subscribe: ({ sessionId }, listener) =>
      sessionId
        ? subscribeCopilotCommands(sessionId, listener)
        : () => undefined,
  },
  sendTurn: sendCopilotTurn,
  compactContext: compactCopilotContext,
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
