import { composeToolTitle } from "./harness/preview";
import { isInFlightSession } from "./inFlight";
import { displayPath } from "./paths";
import {
  sessionDisplayTitle,
  type Block,
  type HarnessId,
  type Session,
} from "./session";

export type LiveAgent = {
  id: string;
  cwd: string;
  title: string;
  harness: HarnessId;
  activity: string;
  startedAt?: number;
  durationMs?: number;
  needsApproval: boolean;
  done: boolean;
};

export function liveAgentsFromSessions(
  sessions: Session[],
  unseenFinishedIds: ReadonlySet<string> = new Set(),
): LiveAgent[] {
  return sessions
    .filter(
      (session) =>
        !session.inboxAsk && (isInFlightSession(session) || unseenFinishedIds.has(session.id)),
    )
    .map((session) =>
      toLiveAgent(session, unseenFinishedIds.has(session.id)),
    )
    .sort(compareLiveAgents);
}

export function formatLiveElapsed(startedAt: number, now: number): string {
  const seconds = Math.max(1, Math.round((now - startedAt) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  if (minutes < 60) return rest ? `${minutes}m ${rest}s` : `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const minRest = minutes % 60;
  return minRest ? `${hours}h ${minRest}m` : `${hours}h`;
}

function toLiveAgent(session: Session, unseenFinished: boolean): LiveAgent {
  const pending = session.blocks.find(
    (block) => block.approval && !block.approval.decided,
  );
  const pendingQuestion = session.pendingQuestion;
  const done = unseenFinished && !isInFlightSession(session);
  const activityBlock = pending ?? lastActivityBlock(session.blocks);
  return {
    id: session.id,
    cwd: session.cwd,
    title: sessionDisplayTitle(session.title, session.harness),
    harness: session.harness,
    activity: done
      ? "Done"
      : pendingQuestion
        ? pendingQuestion.title ||
          pendingQuestion.questions[0]?.prompt ||
          "Question"
        : activityLabel(activityBlock, session.cwd),
    startedAt: turnStartedAt(session.blocks),
    durationMs: done ? turnDurationMs(session.blocks) : undefined,
    needsApproval: Boolean(pending) || Boolean(pendingQuestion),
    done,
  };
}

function compareLiveAgents(a: LiveAgent, b: LiveAgent): number {
  if (a.needsApproval !== b.needsApproval) return a.needsApproval ? -1 : 1;
  if (a.done !== b.done) return a.done ? 1 : -1;
  return (
    (a.startedAt ?? Number.MAX_SAFE_INTEGER) -
    (b.startedAt ?? Number.MAX_SAFE_INTEGER)
  );
}

function lastActivityBlock(blocks: Block[]): Block | undefined {
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i];
    if (block.role === "tool" || block.role === "approval") return block;
    if (block.role === "handoff" && block.handoff?.status === "preparing") {
      return block;
    }
  }
  return undefined;
}

function turnStartedAt(blocks: Block[]): number | undefined {
  for (let i = blocks.length - 1; i >= 0; i--) {
    if (blocks[i].role === "user") return blocks[i].startedAt;
  }
  return undefined;
}

function turnDurationMs(blocks: Block[]): number | undefined {
  for (let i = blocks.length - 1; i >= 0; i--) {
    if (blocks[i].role === "user") return blocks[i].durationMs;
  }
  return undefined;
}

function activityLabel(block: Block | undefined, cwd: string): string {
  if (!block) return "Working";
  if (block.role === "handoff" && block.handoff?.status === "preparing") {
    return "Preparing a handoff";
  }
  const preview = block.tool?.preview;
  const path = preview?.path
    ? displayPath(preview.path, cwd)
    : preview?.fileName;
  return (
    composeToolTitle({
      kind: block.tool?.kind,
      title: block.text || block.tool?.title,
      path,
      query: preview?.query,
      previewKind: preview?.kind,
      cwd,
    }) || "Working"
  );
}
