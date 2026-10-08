// Adapted from AmeinEskinder, https://github.com/hardbeat920/monocode/pull/565.
// Unlike that revision, retained/reused workers validate existing history before dispatch;
// shared workers keep their previous edit-tracking behavior.
import type { OrchestrationTask } from "../../features/orchestration/model/orchestrationState";
import {
  captureSessionCheckpoint,
  ensureWorkerCheckpoint,
  captureWorkerCheckpoint,
  flushSessionCheckpoint,
  notifyReviewChanged,
  prepareSessionCheckpoint,
} from "../../features/sessions/model/checkpoint";
import { isEditTool } from "../../integrations/harness/core/preview";
import type { HarnessEvent } from "../../integrations/harness/core/types";

/** Seed a new isolated checkout before the worker can edit it. */
export async function prepareWorkerCheckpoint(
  task: Pick<OrchestrationTask, "sessionId" | "workspace" | "workspacePolicy">,
  checkoutCwd: string,
  discardNewCheckout?: () => Promise<unknown>,
  createdCheckout = !task.workspace,
): Promise<void> {
  if (task.workspacePolicy === "shared") return;
  try {
    await ensureWorkerCheckpoint(task.sessionId, checkoutCwd, !createdCheckout);
  } catch (error) {
    if (createdCheckout && discardNewCheckout) {
      try {
        await discardNewCheckout();
      } catch (cleanupError) {
        throw new Error(
          `Checkpoint preparation failed: ${String(error)}. Checkout cleanup failed: ${String(cleanupError)}. The worktree was kept.`,
        );
      }
    }
    throw error;
  }
}

export async function settleWorkerCheckpoint(
  task: Pick<OrchestrationTask, "sessionId" | "files">,
  cwd: string,
  stop: () => Promise<void>,
): Promise<void> {
  await stop();
  await flushSessionCheckpoint(task.sessionId);
  await captureWorkerCheckpoint(task.sessionId, cwd, task.files);
  notifyReviewChanged(task.sessionId);
}

export function trackSessionEdits(
  sessionId: string,
  cwd: string,
  event: HarnessEvent,
  orchestrationLeadId?: string,
  workspacePolicy?: OrchestrationTask["workspacePolicy"],
) {
  if (sessionId === orchestrationLeadId || workspacePolicy === "shared") return;
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
