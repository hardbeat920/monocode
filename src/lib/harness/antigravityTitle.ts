import {
  buildThreadTitlePrompt,
  parseGeneratedThreadTitle,
} from "../sessionTitle";
import { runAntigravityTextPrompt } from "./antigravityText";

const TITLE_TIMEOUT_MS = 60_000;

export async function generateAntigravitySessionTitle(input: {
  sessionId: string;
  cwd: string;
  message: string;
}): Promise<string | null> {
  try {
    const output = await runAntigravityTextPrompt({
      cwd: input.cwd,
      prompt: buildThreadTitlePrompt(input.message),
      timeoutMs: TITLE_TIMEOUT_MS,
    });
    return parseGeneratedThreadTitle(output);
  } catch (error) {
    console.debug("[monocode] session title", error);
    return null;
  }
}
