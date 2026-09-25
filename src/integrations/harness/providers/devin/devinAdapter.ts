import {
  bindDevinSession,
  cancelDevinTurn,
  forgetDevinSession,
  respondDevinApproval,
  sendDevinTurn,
  steerDevinTurn,
  stopDevinSession,
} from "./devin";
import { refreshDevinCatalog } from "./devinCatalog";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

export const devinAdapter: HarnessAdapter = {
  id: "devin",
  live: true,
  canSteer: false,
  sendTurn: sendDevinTurn,
  steerTurn: steerDevinTurn,
  cancelTurn: cancelDevinTurn,
  respondApproval: respondDevinApproval,
  stopSession: stopDevinSession,
  forgetSession: forgetDevinSession,
  bindSession: bindDevinSession,
  refreshCatalog: refreshDevinCatalog,
};

let registered = false;

export function ensureDevinRegistered(): void {
  if (registered) return;
  registerHarness(devinAdapter);
  registered = true;
}
