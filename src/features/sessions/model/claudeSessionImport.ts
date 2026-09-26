import { applyHarnessEvent } from "../../../integrations/harness/core/apply";
import { replayClaudeSession } from "../../../integrations/harness/providers/claude/claude";
import { sessionWorkCwd, type Block, type Session } from "./session";

/** The thread a conversation was picked for, as it looked when the picker opened. */
export type ClaudeImportTarget = {
  sessionId: string;
  /** Directory the conversations were listed for; the working copy, not the project. */
  cwd: string;
  providerAccountId?: string;
};

/**
 * The thread an import may still be committed to, or `null`.
 *
 * Loading replaces the thread's transcript, and the steps in between — reading
 * half a megabyte of session file, stopping the live child — are each a round
 * trip during which the thread can be closed, start a turn, or move to another
 * provider, account or working copy. None of those should have their content
 * overwritten, and the directory matters beyond that: the conversation was
 * listed for `cwd`, and Claude drops a resume binding whose directory is not
 * the one the next turn runs in, so a thread that has since moved would start
 * a new conversation rather than continue the chosen one.
 */
export function claudeImportTarget(
  sessions: Session[],
  target: ClaudeImportTarget,
): Session | null {
  const session = sessions.find((entry) => entry.id === target.sessionId);
  if (
    !session ||
    session.busy ||
    session.harness !== "claude" ||
    sessionWorkCwd(session) !== target.cwd ||
    session.providerAccountId !== target.providerAccountId
  ) {
    return null;
  }
  return session;
}

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
  // The conversation being loaded *is* the thread's transcript now, so the
  // replay starts from an empty one. Appending instead would leave the target
  // showing two unrelated conversations back to back.
  let session: Session = { ...input.base, blocks: [] };
  replayClaudeSession({
    sessionId: input.base.id,
    // Replay records the resume binding itself, from this directory, as the
    // live stream does. It has to be the working copy: `cwd` is the project
    // for a worktree session, and a binding under the project is dropped when
    // the next turn runs in the checkout.
    cwd: sessionWorkCwd(input.base),
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
