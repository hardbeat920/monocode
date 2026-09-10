import {
  buildThreadTitlePrompt,
  parseGeneratedSessionTitle,
  type GeneratedSessionTitle,
} from "../sessionTitle";
import { runCursorTextPrompt } from "./cursorText";

const TITLE_TIMEOUT_MS = 45_000;

export function stopCursorTitleGeneration(_sessionId: string): Promise<void> {
  return Promise.resolve();
}

/** Cursor ACP turn that returns a sidebar title, or null on failure. */
export async function generateCursorSessionTitle(input: {
  sessionId: string;
  cwd: string;
  message: string;
}): Promise<GeneratedSessionTitle | null> {
  try {
    const output = await runCursorTextPrompt({
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
