import {
  bindKimiSession,
  cancelKimiTurn,
  forgetKimiSession,
  respondKimiApproval,
  sendKimiTurn,
  steerKimiTurn,
  stopKimiSession,
} from "./kimi";
import { refreshKimiCatalog } from "./kimiCatalog";
import { registerHarness, type HarnessAdapter } from "./registry";

export const kimiAdapter: HarnessAdapter = {
  id: "kimi",
  live: true,
  canSteer: false,
  sendTurn: sendKimiTurn,
  steerTurn: steerKimiTurn,
  cancelTurn: cancelKimiTurn,
  respondApproval: respondKimiApproval,
  stopSession: stopKimiSession,
  forgetSession: forgetKimiSession,
  bindSession: bindKimiSession,
  refreshCatalog: refreshKimiCatalog,
};

let registered = false;

export function ensureKimiRegistered(): void {
  if (registered) return;
  registerHarness(kimiAdapter);
  registered = true;
}
