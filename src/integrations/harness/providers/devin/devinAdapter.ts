import type { NativeCommandProvider } from "../../core/nativeCommands";
import {
  bindDevinSession,
  cancelDevinTurn,
  compactDevinContext,
  devinSessionCommands,
  forgetDevinSession,
  respondDevinApproval,
  sendDevinTurn,
  steerDevinTurn,
  stopDevinSession,
  subscribeDevinCommands,
  waitForDevinSessionTitle,
} from "./devin";
import { refreshDevinCatalog } from "./devinCatalog";
import { runDevinTextPrompt, stopDevinTextPrompt } from "./devinText";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

/** Devin's built-in slash commands, as advertised by its live session. */
export const devinCommandProvider: NativeCommandProvider = {
  async discover(context) {
    return context.sessionId ? (devinSessionCommands(context.sessionId) ?? []) : [];
  },
  subscribe(context, onCommands) {
    const current = context.sessionId
      ? devinSessionCommands(context.sessionId)
      : undefined;
    if (current) onCommands(current);
    return subscribeDevinCommands((threadId, commands) => {
      if (threadId === context.sessionId) onCommands(commands);
    });
  },
};

export const devinAdapter: HarnessAdapter = {
  id: "devin",
  live: true,
  canSteer: false,
  commands: devinCommandProvider,
  sendTurn: sendDevinTurn,
  compactContext: compactDevinContext,
  steerTurn: steerDevinTurn,
  cancelTurn: cancelDevinTurn,
  respondApproval: respondDevinApproval,
  stopSession: stopDevinSession,
  forgetSession: forgetDevinSession,
  bindSession: bindDevinSession,
  refreshCatalog: refreshDevinCatalog,
  async generateTitle({ sessionId }) {
    const title = await waitForDevinSessionTitle(sessionId);
    return title ? { title, workItem: null } : null;
  },
  runTextPrompt: runDevinTextPrompt,
  stopTextPrompt: stopDevinTextPrompt,
};

let registered = false;

export function ensureDevinRegistered(): void {
  if (registered) return;
  registerHarness(devinAdapter);
  registered = true;
}
