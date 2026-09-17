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

/** Adapter registration for the mcode harness — wires live + persistence callbacks to the harness registry. */
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

/** Idempotently register the mcode adapter with the harness registry. */
export function ensureMcodeRegistered(): void {
  if (registered) return;
  registerHarness(mcodeAdapter);
  registered = true;
}
