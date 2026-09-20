import {
  buildThreadTitlePrompt,
  parseGeneratedSessionTitle,
  type GeneratedSessionTitle,
} from "../../../../features/sessions/model/sessionTitle";
import { runClaudeTextPrompt } from "./claudeText";

const TITLE_TIMEOUT_MS = 45_000;

export async function generateClaudeSessionTitle(input: {
  sessionId: string;
  cwd: string;
  message: string;
  providerAccountId?: string;
}): Promise<GeneratedSessionTitle | null> {
  try {
    const output = await runClaudeTextPrompt({
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
