import { applyHarnessEvent } from "../../../integrations/harness/core/apply";
import { replayClaudeSession } from "../../../integrations/harness/providers/claude/claude";
import type { Block, Session } from "./session";

/**
 * Rebuild a MonoCode session from a conversation Claude Code stored on disk.
 *
 * Replay feeds the stored records through the same handler the live stream
 * uses, so replies, tool rows and subagent steps come out identical to the way
 * they first appeared. Prompts are the exception: live they are blocks the
 * composer adds when the turn is sent, never events, so they are appended here.
 */
export function buildImportedSession(input: {
  base: Session;
  transcript: string;
  providerSessionId: string;
  providerAccountId?: string;
}): Session {
  let session = input.base;
  replayClaudeSession({
    sessionId: input.base.id,
    cwd: input.base.cwd,
    providerAccountId: input.providerAccountId,
    runtimeMode: input.base.runtimeMode,
    transcript: input.transcript,
    onEvent: (event) => {
      session = applyHarnessEvent(session, event);
    },
    onPrompt: (text, at) => {
      session = appendPrompt(session, text, at);
    },
  });
  return {
    ...session,
    // Binding the conversation is what makes the next turn continue it rather
    // than start something new.
    providerSessionId: input.providerSessionId,
    ...(input.providerAccountId
      ? { providerAccountId: input.providerAccountId }
      : {}),
  };
}

function appendPrompt(
  session: Session,
  text: string,
  at: number | undefined,
): Session {
  const block: Block = {
    id: crypto.randomUUID(),
    role: "user",
    text,
    ...(at ? { startedAt: at } : {}),
  };
  return {
    ...session,
    blocks: [
      ...session.blocks.map((b) => (b.streaming ? { ...b, streaming: false } : b)),
      block,
    ],
  };
}
