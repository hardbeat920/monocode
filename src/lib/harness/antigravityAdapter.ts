import {
  bindAntigravitySession,
  cancelAntigravityTurn,
  forgetAntigravitySession,
  respondAntigravityApproval,
  sendAntigravityTurn,
  steerAntigravityTurn,
  stopAntigravitySession,
} from "./antigravity";
import { refreshAntigravityCatalog } from "./antigravityCatalog";
import { registerHarness, type HarnessAdapter } from "./registry";

export const antigravityAdapter: HarnessAdapter = {
  id: "antigravity",
  live: true,
  canSteer: false,
  sendTurn: sendAntigravityTurn,
  steerTurn: steerAntigravityTurn,
  cancelTurn: cancelAntigravityTurn,
  respondApproval: respondAntigravityApproval,
  stopSession: stopAntigravitySession,
  forgetSession: forgetAntigravitySession,
  bindSession: bindAntigravitySession,
  refreshCatalog: refreshAntigravityCatalog,
};

let registered = false;

export function ensureAntigravityRegistered(): void {
  if (registered) return;
  registerHarness(antigravityAdapter);
  registered = true;
}
