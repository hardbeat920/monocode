import { ask } from "@tauri-apps/plugin-dialog";
import type { HarnessEvent } from "../lib/harness";
import { isEditTool } from "../lib/harness/preview";
import {
  captureSessionCheckpoint,
  notifyReviewChanged,
  prepareSessionCheckpoint,
} from "../lib/checkpoint";
import { invalidateProjectFiles } from "../lib/fileIndex";
import { notifyDirsChanged } from "../lib/fileTree";
import { nudgeWatchedFiles } from "../lib/fileWatch";
import { notifyGitChanged } from "../lib/fs";
import { resolveWorkspacePath } from "../lib/paths";

export type ScheduledFlush = { kind: "raf" | "timeout"; id: number };

export function cancelScheduledFlush(handle: ScheduledFlush | null) {
  if (!handle) return;
  if (handle.kind === "raf") cancelAnimationFrame(handle.id);
  else clearTimeout(handle.id);
}

export function scheduleHarnessFlush(run: () => void): ScheduledFlush {
  if (document.hidden) {
    return { kind: "timeout", id: window.setTimeout(run, 32) };
  }
  return { kind: "raf", id: requestAnimationFrame(run) };
}

/** Native sheet. `window.confirm` is swallowed when a macOS menu accelerator fires. */
export function confirmDiscardUnsaved(message: string): Promise<boolean> {
  return ask(message, { title: "MonoCode", kind: "warning" });
}

export function sameSettings(
  a: Record<string, string> | undefined,
  b: Record<string, string> | undefined,
): boolean {
  const left = a ?? {};
  const right = b ?? {};
  const keys = new Set([...Object.keys(left), ...Object.keys(right)]);
  for (const key of keys) {
    if (left[key] !== right[key]) return false;
  }
  return true;
}

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

export function nudgeWorkspace(cwd?: string) {
  invalidateProjectFiles(cwd);
  notifyDirsChanged();
}

export function nudgeOpenEditors(event: HarnessEvent, cwd: string) {
  if (event.type !== "tool.updated") return;
  const completed = event.status === "completed" || event.status === "success";

  const kind = event.kind?.trim().toLowerCase();
  if (kind === "execute" || event.preview?.kind === "shell") {
    if (!completed) return;
    nudgeWatchedFiles();
    window.setTimeout(() => nudgeWatchedFiles(), 150);
    notifyGitChanged();
    nudgeWorkspace(cwd);
    window.setTimeout(() => nudgeWorkspace(cwd), 150);
    return;
  }

  if (!isEditTool(event.kind, event.title, event.preview)) return;
  const raw = event.preview?.path;
  const resolved = raw ? (resolveWorkspacePath(raw, cwd) ?? raw) : undefined;
  if (resolved) {
    nudgeWatchedFiles([resolved]);
  } else if (completed) {
    nudgeWatchedFiles();
  }
  if (completed) {
    window.setTimeout(() => nudgeWatchedFiles(), 150);
    notifyGitChanged();
    nudgeWorkspace(cwd);
  }
}
