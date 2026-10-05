import { appendReadyHandoff, buildDeterministicHandoff } from "./handoff";
import type { Session } from "./session";

/**
 * Promote a projectless chat into a project, keeping its transcript.
 *
 * A provider conversation is bound to the folder it started in, so the thread
 * cannot simply resume from the project. The chat starts a fresh provider
 * thread there instead, and a brief of the conversation so far rides along
 * with the next turn so the agent still knows what was discussed.
 */
export function moveChatToProject(session: Session, cwd: string): Session {
  const brief = buildDeterministicHandoff(session);
  const moved: Session = {
    ...session,
    cwd,
    providerSessionId: undefined,
    context: undefined,
    branch: undefined,
    worktreeCwd: undefined,
    worktreeRemoved: undefined,
    workspaceMode: undefined,
    worktreeBase: undefined,
  };
  return brief.trim()
    ? appendReadyHandoff(moved, session.harness, session.harness, brief)
    : moved;
}
