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
import { ompCommandProvider, respondQuestion } from "./piFamily";
import { OMP_FLAVOR } from "./piFlavor";

export const ompAdapter: HarnessAdapter = {
  id: "omp",
  live: true,
  commands: ompCommandProvider,
  respondQuestion: (sessionId, requestId, reply) =>
    respondQuestion(OMP_FLAVOR, sessionId, requestId, reply),
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
