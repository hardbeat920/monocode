import {
  bindMcodeSession,
  cancelMcodeTurn,
  forgetMcodeSession,
  respondMcodeApproval,
  sendMcodeTurn,
  steerMcodeTurn,
  stopMcodeSession,
} from "./mcode";
import { registerHarness, type HarnessAdapter } from "./registry";

export const mcodeAdapter: HarnessAdapter = {
  id: "mcode",
  live: true,
  canSteer: false,
  sendTurn: sendMcodeTurn,
  steerTurn: steerMcodeTurn,
  cancelTurn: cancelMcodeTurn,
  respondApproval: respondMcodeApproval,
  stopSession: stopMcodeSession,
  forgetSession: forgetMcodeSession,
  bindSession: bindMcodeSession,
};

let registered = false;

export function ensureMcodeRegistered(): void {
  if (registered) return;
  registerHarness(mcodeAdapter);
  registered = true;
}
