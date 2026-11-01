import type { ChangedFile } from "@/features/workspace/types";

import type { InboxCheck, InboxDiffFile } from "./protocol";

/** A file of the item's diff as the diff screen reads it. */
export function toChangedFile(file: InboxDiffFile): ChangedFile {
  return {
    relative: file.path,
    status: "modified",
    additions: file.additions,
    deletions: file.deletions,
    lines: file.lines.map((line) => ({
      kind: line.kind,
      text: line.text,
      oldNumber: line.oldNo ?? null,
      newNumber: line.newNo ?? null,
    })),
  };
}


/** "failed · 6m 40s", "passed · 41s": a check's outcome and how long it took. */
export function checkLabel(check: InboxCheck) {
  const word =
    check.state === "failing"
      ? "failed"
      : check.state === "pending"
        ? "running"
        : check.state === "skipped"
          ? "skipped"
          : "";
  const time = check.durationMs !== undefined ? duration(check.durationMs) : "";
  return [word, time].filter(Boolean).join(" · ");
}

function duration(ms: number) {
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return rest ? `${minutes}m ${String(rest).padStart(2, "0")}s` : `${minutes}m`;
}
