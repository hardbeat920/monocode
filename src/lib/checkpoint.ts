import { invoke } from "@tauri-apps/api/core";
import type { GitDiffStats, GitFileDiff } from "./fs";

export type CheckpointFile = {
  path: string;
  relative: string;
  status: string;
  additions: number;
  deletions: number;
};

export type CheckpointStatus = {
  files: CheckpointFile[];
};

const REVIEW_CHANGED = "monocode-review-changed";

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
  return invoke<void>("session_checkpoint_ensure", { sessionId, cwd });
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

export function captureSessionCheckpoint(
  sessionId: string,
  cwd: string,
  paths: string[],
): Promise<void> {
  if (paths.length === 0) return Promise.resolve();
  return invoke<void>("session_checkpoint_capture", {
    sessionId,
    cwd,
    paths,
  });
}

export function syncSessionCheckpoint(
  sessionId: string,
  cwd: string,
): Promise<void> {
  if (!cwd || cwd === "~") return Promise.resolve();
  return invoke<void>("session_checkpoint_sync", { sessionId, cwd });
}

export function sessionCheckpointStatus(
  sessionId: string,
  cwd: string,
): Promise<CheckpointStatus> {
  return invoke<CheckpointStatus>("session_checkpoint_status", {
    sessionId,
    cwd,
  });
}

export function sessionCheckpointStats(
  cwd: string,
  sessionIds: string[],
): Promise<Record<string, GitDiffStats>> {
  if (!cwd || cwd === "~" || sessionIds.length === 0) return Promise.resolve({});
  return invoke<Record<string, GitDiffStats>>("session_checkpoint_stats", {
    cwd,
    sessionIds,
  });
}

/** Session baseline vs worktree, so review shows only this session's edits. */
export function sessionFileDiff(
  sessionId: string,
  cwd: string,
  relative: string,
): Promise<GitFileDiff> {
  return invoke<GitFileDiff>("session_checkpoint_file_diff", {
    sessionId,
    cwd,
    relative,
  });
}

export function undoSessionChanges(
  sessionId: string,
  cwd: string,
  relative?: string,
): Promise<CheckpointStatus> {
  return invoke<CheckpointStatus>("session_checkpoint_undo", {
    sessionId,
    cwd,
    relative: relative ?? null,
  });
}

export function keepSessionChanges(
  sessionId: string,
  cwd: string,
  relative?: string,
): Promise<CheckpointStatus> {
  return invoke<CheckpointStatus>("session_checkpoint_keep", {
    sessionId,
    cwd,
    relative: relative ?? null,
  });
}
