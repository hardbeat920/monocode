import { invoke } from "@tauri-apps/api/core";

export type CheckpointFile = {
  path: string;
  relative: string;
  status: string;
  additions: number;
  deletions: number;
  /** False when exact line ownership overlaps another session. */
  exact: boolean;
  /** False when restoring could overwrite a change made outside this session. */
  undoable: boolean;
};

export type CheckpointStatus = {
  files: CheckpointFile[];
};

export type CheckpointFileDiff = {
  path: string;
  relative: string;
  status: string;
  original: string;
  current: string;
  binary: boolean;
  tooLarge: boolean;
};

const REVIEW_CHANGED = "monocode-review-changed";
const checkpointQueues = new Map<string, Promise<void>>();

function enqueueCheckpoint<T>(
  sessionId: string,
  operation: () => Promise<T>,
): Promise<T> {
  const previous = checkpointQueues.get(sessionId) ?? Promise.resolve();
  const result = previous.then(operation, operation);
  const tail = result.then(
    () => undefined,
    () => undefined,
  );
  checkpointQueues.set(sessionId, tail);
  void tail.then(() => {
    if (checkpointQueues.get(sessionId) === tail) {
      checkpointQueues.delete(sessionId);
    }
  });
  return result;
}

/** Wait until every queued checkpoint write for this session is durable. */
export function flushSessionCheckpoint(sessionId: string): Promise<void> {
  return checkpointQueues.get(sessionId) ?? Promise.resolve();
}

export function notifyReviewChanged(sessionId?: string) {
  window.dispatchEvent(
    new CustomEvent(REVIEW_CHANGED, { detail: sessionId ?? "" }),
  );
}

export function subscribeReviewChanged(
  listener: (sessionId: string) => void,
): () => void {
  const handler = (event: Event) => {
    listener((event as CustomEvent<string>).detail ?? "");
  };
  window.addEventListener(REVIEW_CHANGED, handler);
  return () => window.removeEventListener(REVIEW_CHANGED, handler);
}

export function ensureSessionCheckpoint(
  sessionId: string,
  cwd: string,
): Promise<void> {
  return enqueueCheckpoint(sessionId, () =>
    invoke<void>("session_checkpoint_ensure", { sessionId, cwd }),
  );
}

/** Snapshot the worktree before a live turn so Keep/Undo can target this session. */
export async function beginSessionTurn(
  sessionId: string,
  cwd: string,
): Promise<void> {
  if (!cwd || cwd === "~") return;
  await ensureSessionCheckpoint(sessionId, cwd);
  notifyReviewChanged(sessionId);
}

/** Capture a file immediately before a structured edit starts. */
export function prepareSessionCheckpoint(
  sessionId: string,
  cwd: string,
  paths: string[],
): Promise<void> {
  if (paths.length === 0) return Promise.resolve();
  return enqueueCheckpoint(sessionId, () =>
    invoke<void>("session_checkpoint_prepare", {
      sessionId,
      cwd,
      paths,
    }),
  );
}

export function captureSessionCheckpoint(
  sessionId: string,
  cwd: string,
  paths: string[],
): Promise<void> {
  if (paths.length === 0) return Promise.resolve();
  return enqueueCheckpoint(sessionId, () =>
    invoke<void>("session_checkpoint_capture", {
      sessionId,
      cwd,
      paths,
    }),
  );
}

export function sessionCheckpointStatus(
  sessionId: string,
  cwd: string,
): Promise<CheckpointStatus> {
  return enqueueCheckpoint(sessionId, () =>
    invoke<CheckpointStatus>("session_checkpoint_status", {
      sessionId,
      cwd,
    }),
  );
}

/** The exact before/after contents captured for one session-owned file. */
export function sessionCheckpointFileDiff(
  sessionId: string,
  cwd: string,
  relative: string,
): Promise<CheckpointFileDiff> {
  return enqueueCheckpoint(sessionId, () =>
    invoke<CheckpointFileDiff>("session_checkpoint_file_diff", {
      sessionId,
      cwd,
      relative,
    }),
  );
}

export function undoSessionChanges(
  sessionId: string,
  cwd: string,
  relative?: string,
): Promise<CheckpointStatus> {
  return enqueueCheckpoint(sessionId, () =>
    invoke<CheckpointStatus>("session_checkpoint_undo", {
      sessionId,
      cwd,
      relative: relative ?? null,
    }),
  );
}

export function keepSessionChanges(
  sessionId: string,
  cwd: string,
  relative?: string,
): Promise<CheckpointStatus> {
  return enqueueCheckpoint(sessionId, () =>
    invoke<CheckpointStatus>("session_checkpoint_keep", {
      sessionId,
      cwd,
      relative: relative ?? null,
    }),
  );
}
