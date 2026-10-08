import { isEditTool } from "../../integrations/harness/core/preview";
import type { HarnessEvent } from "../../integrations/harness/core/types";
import {
  captureSessionCheckpoint,
  notifyReviewChanged,
  prepareSessionCheckpoint,
} from "../../features/sessions/model/checkpoint";

export function createSessionEditTracker(sessionId: string, cwd: string) {
  // One tracker per turn: call IDs and unfinished diffs never cross turns.
  const startedDiffs = new Map<string, Record<string, string>>();
  return (event: HarnessEvent) => {
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
      // Progress is not another edit boundary. Complete provider diffs also avoid
      // racing an asynchronous snapshot against the provider's file write.
      if (event.type !== "tool.started") return;
      if (event.checkpointDiffs) {
        startedDiffs.set(event.callId, event.checkpointDiffs);
      } else {
        startedDiffs.delete(event.callId);
      }
      const patchedPaths = Object.keys(event.checkpointDiffs ?? {});
      const snapshotPaths = paths.filter(
        (path) => !patchedPaths.includes(path),
      );
      void prepareSessionCheckpoint(sessionId, cwd, snapshotPaths).catch(
        () => undefined,
      );
      return;
    }
    const fallback = startedDiffs.get(event.callId);
    startedDiffs.delete(event.callId);
    // Completion is authoritative per path; retain skipped snapshots' diffs.
    const diffs = fallback
      ? { ...fallback, ...event.checkpointDiffs }
      : event.checkpointDiffs;
    void captureSessionCheckpoint(sessionId, cwd, paths, diffs)
      .catch(() => undefined)
      .then(() => notifyReviewChanged(sessionId));
  };
}
