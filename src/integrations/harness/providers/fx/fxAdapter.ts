import {
  bindFxSession,
  cancelFxTurn,
  forgetFxSession,
  respondFxApproval,
  sendFxTurn,
  steerFxTurn,
  stopFxSession,
} from "./fx";
import { refreshFxCatalog } from "./fxCatalog";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

export const fxAdapter: HarnessAdapter = {
  id: "fx",
  live: true,
  canSteer: false,
  sendTurn: sendFxTurn,
  steerTurn: steerFxTurn,
  cancelTurn: cancelFxTurn,
  respondApproval: respondFxApproval,
  stopSession: stopFxSession,
  forgetSession: forgetFxSession,
  bindSession: bindFxSession,
  refreshCatalog: refreshFxCatalog,
};

let registered = false;

export function ensureFxRegistered(): void {
  if (registered) return;
  registerHarness(fxAdapter);
  registered = true;
}
