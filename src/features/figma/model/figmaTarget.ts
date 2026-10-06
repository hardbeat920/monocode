import {
  isRemoteProjectPath,
  sameProjectPath,
} from "../../projects/model/recents";
import {
  sessionDisplayTitle,
  sessionWorkCwd,
  type HarnessId,
  type Session,
} from "../../sessions/model/session";

export type FigmaSessionTarget = {
  sessionId: string;
  cwd: string;
  workCwd: string;
  title: string;
  harness: HarnessId;
  model: string;
  modelSettings: Record<string, string>;
  busy: boolean;
};

let target: FigmaSessionTarget | null = null;
const listeners = new Set<() => void>();

export function figmaSessionTargetFrom(
  session: Session | null | undefined,
): FigmaSessionTarget | null {
  if (
    !session ||
    session.inboxAsk ||
    session.worktreeRemoved ||
    session.worktreePreparing ||
    (session.workspaceMode === "worktree" && !session.worktreeCwd) ||
    session.orchestrationLeadId ||
    isRemoteProjectPath(session.cwd)
  )
    return null;
  return {
    sessionId: session.id,
    cwd: session.cwd,
    workCwd: sessionWorkCwd(session),
    title: sessionDisplayTitle(session.title, session.harness),
    harness: session.harness,
    model: session.model,
    modelSettings: session.modelSettings,
    busy: !!session.busy,
  };
}

export function figmaSessionTargetFor(
  project: string,
): FigmaSessionTarget | null {
  return target && sameProjectPath(target.cwd, project) ? target : null;
}

export function setFigmaSessionTarget(next: FigmaSessionTarget | null): void {
  if (sameTarget(target, next)) return;
  target = next;
  for (const listener of listeners) listener();
}

export function subscribeFigmaSessionTarget(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function sameSettings(
  left: Record<string, string>,
  right: Record<string, string>,
): boolean {
  const keys = Object.keys(left);
  return (
    keys.length === Object.keys(right).length &&
    keys.every((key) => left[key] === right[key])
  );
}

function sameTarget(
  left: FigmaSessionTarget | null,
  right: FigmaSessionTarget | null,
): boolean {
  if (!left || !right) return left === right;
  return (
    left.sessionId === right.sessionId &&
    left.cwd === right.cwd &&
    left.workCwd === right.workCwd &&
    left.title === right.title &&
    left.harness === right.harness &&
    left.model === right.model &&
    left.busy === right.busy &&
    sameSettings(left.modelSettings, right.modelSettings)
  );
}
