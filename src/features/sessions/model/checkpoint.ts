import { invoke } from "@tauri-apps/api/core";
import { isEditTool } from "../../../integrations/harness/core/preview";
import type { HarnessEvent } from "../../../integrations/harness/core/types";

export type CheckpointFile = {
  path: string;
  relative: string;
  status: string;
  additions: number;
  deletions: number;
  /** False when changes between this session's edits prevent an exact diff. */
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

export type CheckpointApplyResult = {
  files: string[];
  alreadyApplied: number;
  /** Changed files outside the write scopes, left in the worker worktree. */
  skipped: string[];
  /** Gitignored files the worker created that the lead lacks, not applied. */
  ignored?: string[];
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

/**
 * Record the session's starting state. An isolated worker owns its checkout,
 * so every later change there, including shell edits, counts as its own.
 */
export function ensureSessionCheckpoint(
  sessionId: string,
  cwd: string,
  isolated = false,
): Promise<void> {
  return enqueueCheckpoint(sessionId, () =>
    invoke<void>("session_checkpoint_ensure", {
      sessionId,
      cwd,
      ...(isolated ? { isolated } : {}),
    }),
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

/** Record the files an edit tool touches: before it starts and after it completes. */
export function trackSessionEdits(
  sessionId: string,
  cwd: string,
  event: HarnessEvent,
) {
  if (event.type !== "tool.started" && event.type !== "tool.updated") return;
  if (!isEditTool(event.kind, event.title, event.preview)) return;
  const paths = [
    ...(event.paths ?? []),
    ...(event.preview?.path ? [event.preview.path] : []),
  ].filter((path, index, all) => all.indexOf(path) === index);
  if (paths.length === 0 || cwd === "~") return;
  const completed =
    event.type === "tool.updated" &&
    (event.status === "completed" || event.status === "success");
  if (!completed) {
    void prepareSessionCheckpoint(sessionId, cwd, paths).catch(() => undefined);
    return;
  }
  void captureSessionCheckpoint(sessionId, cwd, paths)
    .catch(() => undefined)
    .then(() => notifyReviewChanged(sessionId));
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

/**
 * Apply one isolated worker's delta to its lead checkout. When writeScopes is
 * given, changed files outside every scope are left in the worker worktree
 * and listed in `skipped`.
 */
export function applySessionCheckpoint(
  sessionId: string,
  fromCwd: string,
  toCwd: string,
  writeScopes?: string[],
): Promise<CheckpointApplyResult> {
  return enqueueCheckpoint(sessionId, () =>
    invoke<CheckpointApplyResult>("session_checkpoint_apply", {
      sessionId,
      fromCwd,
      toCwd,
      ...(writeScopes ? { writeScopes } : {}),
    }),
  );
}

/** True only when the checkout still matches its seeded, pre-worker state. */
export function sessionCheckpointCleanupSafe(
  sessionId: string,
  cwd: string,
): Promise<boolean> {
  return enqueueCheckpoint(sessionId, () =>
    invoke<boolean>("session_checkpoint_cleanup_safe", { sessionId, cwd }),
  );
}

export function forgetSessionCheckpoint(sessionId: string): Promise<void> {
  return enqueueCheckpoint(sessionId, () =>
    invoke<void>("session_checkpoint_forget", { sessionId }),
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
