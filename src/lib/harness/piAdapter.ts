import {
  bindPiSession,
  cancelPiTurn,
  compactPiContext,
  forgetPiSession,
  respondPiApproval,
  sendPiTurn,
  steerPiTurn,
  stopPiSession,
} from "./pi";
import { refreshPiCatalog } from "./piCatalog";
import { generatePiSessionTitle } from "./piTitle";
import { warmupPiText } from "./piText";
import { registerHarness, type HarnessAdapter } from "./registry";

export const piAdapter: HarnessAdapter = {
  id: "pi",
  live: true,
  sendTurn: sendPiTurn,
  compactContext: compactPiContext,
  steerTurn: steerPiTurn,
  cancelTurn: cancelPiTurn,
  respondApproval: respondPiApproval,
  stopSession: stopPiSession,
  forgetSession: forgetPiSession,
  bindSession: bindPiSession,
  refreshCatalog: refreshPiCatalog,
  generateTitle: generatePiSessionTitle,
  warmupText: warmupPiText,
};

let registered = false;

export function ensurePiRegistered(): void {
  if (registered) return;
  registerHarness(piAdapter);
  registered = true;
}
