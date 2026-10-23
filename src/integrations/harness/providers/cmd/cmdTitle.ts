import {
  buildThreadTitlePrompt,
  parseGeneratedSessionTitle,
  type GeneratedSessionTitle,
} from "../../../../features/sessions/model/sessionTitle";
import { runCmdTextPrompt } from "./cmdText";

const TITLE_TIMEOUT_MS = 60_000;

export async function generateCmdSessionTitle(input: {
  sessionId: string;
  cwd: string;
  message: string;
}): Promise<GeneratedSessionTitle | null> {
  try {
    const output = await runCmdTextPrompt({
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
