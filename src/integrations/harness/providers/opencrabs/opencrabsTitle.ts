import {
  buildThreadTitlePrompt,
  parseGeneratedSessionTitle,
  type GeneratedSessionTitle,
} from "../../../../features/sessions/model/sessionTitle";
import { runOpenCrabsTextPrompt } from "./opencrabsText";

const TITLE_TIMEOUT_MS = 45_000;

export async function generateOpenCrabsSessionTitle(input: {
  sessionId: string;
  cwd: string;
  message: string;
}): Promise<GeneratedSessionTitle | null> {
  try {
    const output = await runOpenCrabsTextPrompt({
      cwd: input.cwd,
      prompt: buildThreadTitlePrompt(input.message),
      timeoutMs: TITLE_TIMEOUT_MS,
    });
    return parseGeneratedSessionTitle(output, input.message);
  } catch (error) {
    console.debug("[monocode] opencrabs session title", error);
    return null;
  }
}
