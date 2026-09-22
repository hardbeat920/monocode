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
import { refreshOmpCatalog } from "../pi/piCatalog";
import { generateOmpSessionTitle } from "../pi/piTitle";
import {
  runOmpTextPrompt,
  stopOmpTextPrompt,
  warmupOmpText,
} from "../pi/piText";
import { registerHarness, type HarnessAdapter } from "../../core/registry";
import { ompCommandProvider, respondQuestion } from "../pi/piFamily";
import { OMP_FLAVOR } from "../pi/piFlavor";

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
  runTextPrompt: runOmpTextPrompt,
  stopTextPrompt: stopOmpTextPrompt,
};

let registered = false;

export function ensureOmpRegistered(): void {
  if (registered) return;
  registerHarness(ompAdapter);
  registered = true;
}
