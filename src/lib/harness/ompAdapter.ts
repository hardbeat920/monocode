import {
  bindOmpSession,
  cancelOmpTurn,
  compactOmpContext,
  forgetOmpSession,
  respondOmpApproval,
  sendOmpTurn,
  steerOmpTurn,
  stopOmpSession,
} from "./omp";
import { refreshOmpCatalog } from "./piCatalog";
import { generateOmpSessionTitle } from "./piTitle";
import { warmupOmpText } from "./piText";
import { registerHarness, type HarnessAdapter } from "./registry";

export const ompAdapter: HarnessAdapter = {
  id: "omp",
  live: true,
  sendTurn: sendOmpTurn,
  compactContext: compactOmpContext,
  steerTurn: steerOmpTurn,
  cancelTurn: cancelOmpTurn,
  respondApproval: respondOmpApproval,
  stopSession: stopOmpSession,
  forgetSession: forgetOmpSession,
  bindSession: bindOmpSession,
  refreshCatalog: refreshOmpCatalog,
  generateTitle: generateOmpSessionTitle,
  warmupText: warmupOmpText,
};

let registered = false;

export function ensureOmpRegistered(): void {
  if (registered) return;
  registerHarness(ompAdapter);
  registered = true;
}
