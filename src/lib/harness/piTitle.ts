import {
  buildThreadTitlePrompt,
  parseGeneratedSessionTitle,
  type GeneratedSessionTitle,
} from "../sessionTitle";
import { OMP_FLAVOR, PI_FLAVOR, type PiFlavor } from "./piFlavor";
import { runTextPrompt } from "./piText";

const TITLE_TIMEOUT_MS = 45_000;

async function generateSessionTitle(
  flavor: PiFlavor,
  input: {
    sessionId: string;
    cwd: string;
    message: string;
  },
): Promise<GeneratedSessionTitle | null> {
  try {
    const output = await runTextPrompt(flavor, {
      cwd: input.cwd,
      prompt: buildThreadTitlePrompt(input.message),
      timeoutMs: TITLE_TIMEOUT_MS,
    });
    return parseGeneratedSessionTitle(output, input.message);
  } catch (error) {
    console.debug("[monocode] session title", error);
    return null;
  }
}

export function generatePiSessionTitle(input: {
  sessionId: string;
  cwd: string;
  message: string;
}): Promise<GeneratedSessionTitle | null> {
  return generateSessionTitle(PI_FLAVOR, input);
}

export function generateOmpSessionTitle(input: {
  sessionId: string;
  cwd: string;
  message: string;
}): Promise<GeneratedSessionTitle | null> {
  return generateSessionTitle(OMP_FLAVOR, input);
}
