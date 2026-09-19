import {
  bindHermesSession,
  cancelHermesTurn,
  forgetHermesSession,
  respondHermesApproval,
  sendHermesTurn,
  steerHermesTurn,
  stopHermesSession,
} from "./hermes";
import { refreshHermesCatalog } from "./hermesCatalog";
import { registerHarness, type HarnessAdapter } from "./registry";

export const hermesAdapter: HarnessAdapter = {
  id: "hermes",
  live: true,
  sendTurn: sendHermesTurn,
  steerTurn: steerHermesTurn,
  cancelTurn: cancelHermesTurn,
  respondApproval: respondHermesApproval,
  stopSession: stopHermesSession,
  forgetSession: forgetHermesSession,
  bindSession: bindHermesSession,
  refreshCatalog: refreshHermesCatalog,
};

let registered = false;

export function ensureHermesRegistered(): void {
  if (registered) return;
  registerHarness(hermesAdapter);
  registered = true;
}
