import {
  bindAmpSession,
  cancelAmpTurn,
  forgetAmpSession,
  respondAmpApproval,
  sendAmpTurn,
  steerAmpTurn,
  stopAmpSession,
} from "./amp";
import { registerHarness, type HarnessAdapter } from "./registry";

export const ampAdapter: HarnessAdapter = {
  id: "amp",
  live: true,
  canSteer: true,
  sendTurn: sendAmpTurn,
  steerTurn: steerAmpTurn,
  cancelTurn: cancelAmpTurn,
  respondApproval: respondAmpApproval,
  stopSession: stopAmpSession,
  forgetSession: forgetAmpSession,
  bindSession: bindAmpSession,
};

let registered = false;

export function ensureAmpRegistered(): void {
  if (registered) return;
  registerHarness(ampAdapter);
  registered = true;
}
