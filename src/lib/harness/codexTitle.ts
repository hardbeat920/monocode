import {
  buildThreadTitlePrompt,
  parseGeneratedSessionTitle,
  type GeneratedSessionTitle,
} from "../sessionTitle";
import { runCodexTextPrompt } from "./codexText";

const TITLE_TIMEOUT_MS = 45_000;

/** Codex app-server turn that returns a sidebar title, or null on failure. */
export async function generateCodexSessionTitle(input: {
  sessionId: string;
  cwd: string;
  message: string;
}): Promise<GeneratedSessionTitle | null> {
  try {
    const output = await runCodexTextPrompt({
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
