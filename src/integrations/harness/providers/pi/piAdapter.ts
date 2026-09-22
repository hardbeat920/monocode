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
import { runPiTextPrompt, stopPiTextPrompt, warmupPiText } from "./piText";
import { registerHarness, type HarnessAdapter } from "../../core/registry";
import { discoverPiSkills } from "./piSkills";

export const piAdapter: HarnessAdapter = {
  id: "pi",
  live: true,
  commands: { discover: ({ cwd }) => discoverPiSkills(cwd) },
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
  runTextPrompt: runPiTextPrompt,
  stopTextPrompt: stopPiTextPrompt,
};

let registered = false;

export function ensurePiRegistered(): void {
  if (registered) return;
  registerHarness(piAdapter);
  registered = true;
}
