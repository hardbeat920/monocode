import {
  bindDroidSession,
  cancelDroidTurn,
  forgetDroidSession,
  respondDroidApproval,
  sendDroidTurn,
  steerDroidTurn,
  stopDroidSession,
} from "./droid";
import { refreshDroidCatalog } from "./droidCatalog";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

export const droidAdapter: HarnessAdapter = {
  id: "droid",
  live: true,
  canSteer: false,
  sendTurn: sendDroidTurn,
  steerTurn: steerDroidTurn,
  cancelTurn: cancelDroidTurn,
  respondApproval: respondDroidApproval,
  stopSession: stopDroidSession,
  forgetSession: forgetDroidSession,
  bindSession: bindDroidSession,
  refreshCatalog: refreshDroidCatalog,
};

let registered = false;

export function ensureDroidRegistered(): void {
  if (registered) return;
  registerHarness(droidAdapter);
  registered = true;
}
