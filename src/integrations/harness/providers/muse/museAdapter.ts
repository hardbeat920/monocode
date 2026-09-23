import {
  bindMuseSession,
  cancelMuseTurn,
  forgetMuseSession,
  respondMuseApproval,
  sendMuseTurn,
  steerMuseTurn,
  stopMuseSession,
} from "./muse";
import { refreshMuseCatalog } from "./museCatalog";
import { registerHarness, type HarnessAdapter } from "../../core/registry";

export const museAdapter: HarnessAdapter = {
  id: "muse",
  live: true,
  canSteer: false,
  sendTurn: sendMuseTurn,
  steerTurn: steerMuseTurn,
  cancelTurn: cancelMuseTurn,
  respondApproval: respondMuseApproval,
  stopSession: stopMuseSession,
  forgetSession: forgetMuseSession,
  bindSession: bindMuseSession,
  refreshCatalog: refreshMuseCatalog,
};

let registered = false;

export function ensureMuseRegistered(): void {
  if (registered) return;
  registerHarness(museAdapter);
  registered = true;
}
