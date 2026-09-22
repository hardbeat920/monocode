import {
  buildThreadTitlePrompt,
  parseGeneratedSessionTitle,
  type GeneratedSessionTitle,
} from "../../../../features/sessions/model/sessionTitle";
import { runCodexTextPrompt } from "./codexText";

const TITLE_TIMEOUT_MS = 45_000;

/** Codex app-server turn that returns a sidebar title, or null on failure. */
export async function generateCodexSessionTitle(input: {
  sessionId: string;
  cwd: string;
  message: string;
  providerAccountId?: string;
}): Promise<GeneratedSessionTitle | null> {
  try {
    const output = await runCodexTextPrompt({
      cwd: input.cwd,
      providerAccountId: input.providerAccountId,
      prompt: buildThreadTitlePrompt(input.message),
      timeoutMs: TITLE_TIMEOUT_MS,
    });
    return parseGeneratedSessionTitle(output, input.message);
  } catch (error) {
    console.debug("[monocode] session title", error);
    return null;
  }
}
