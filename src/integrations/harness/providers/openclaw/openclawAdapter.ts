import {
  bindOpenClawSession,
  cancelOpenClawTurn,
  forgetOpenClawSession,
  respondOpenClawApproval,
  sendOpenClawTurn,
  steerOpenClawTurn,
  stopOpenClawSession,
} from "./openclaw";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

export const openClawAdapter: HarnessAdapter = {
  id: "openclaw",
  live: true,
  canSteer: false,
  sendTurn: sendOpenClawTurn,
  steerTurn: steerOpenClawTurn,
  cancelTurn: cancelOpenClawTurn,
  respondApproval: respondOpenClawApproval,
  stopSession: stopOpenClawSession,
  forgetSession: forgetOpenClawSession,
  bindSession: bindOpenClawSession,
};

let registered = false;

export function ensureOpenClawRegistered(): void {
  if (registered) return;
  registerHarness(openClawAdapter);
  registered = true;
}
