import {
  buildThreadTitlePrompt,
  parseGeneratedSessionTitle,
  type GeneratedSessionTitle,
} from "../sessionTitle";
import { runOpenCodeTextPrompt } from "./opencodeText";

const TITLE_TIMEOUT_MS = 45_000;

export async function generateOpenCodeSessionTitle(input: {
  sessionId: string;
  cwd: string;
  message: string;
}): Promise<GeneratedSessionTitle | null> {
  try {
    const output = await runOpenCodeTextPrompt({
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
