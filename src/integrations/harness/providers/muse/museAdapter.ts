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
import {
  getHarness,
  registerHarness,
  type HarnessAdapter,
} from "../../core/registry";

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

export function ensureMuseRegistered(): void {
  if (getHarness("muse")) return;
  registerHarness(museAdapter);
}
