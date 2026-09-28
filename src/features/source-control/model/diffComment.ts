import type { ChatContextItem } from "../../sessions/model/chatContext";
import type { UnifiedLine } from "./unifiedDiff";

export type DiffCommentTarget = {
  path: string;
  line: UnifiedLine;
};

function diffCommentLine({ line }: DiffCommentTarget): number | null {
  return line.kind === "del" ? line.oldNumber : line.newNumber;
}

export function diffCommentLocation(target: DiffCommentTarget): string {
  const number = diffCommentLine(target);
  return number == null ? target.path : `${target.path}:${number}`;
}

/** A context chip for a comment on one diff line, or null for an empty comment. */
export function diffCommentContext(
  target: DiffCommentTarget,
  comment: string,
): ChatContextItem | null {
  const body = comment.replace(/\r\n?/g, "\n").trim();
  if (!body) return null;

  const line = diffCommentLine(target);
  return {
    kind: "comment",
    path: target.path.replace(/\\/g, "/"),
    ...(line != null ? { line } : {}),
    change:
      target.line.kind === "add"
        ? "added"
        : target.line.kind === "del"
          ? "removed"
          : "unchanged",
    code: target.line.text.replace(/\r$/, ""),
    comment: body,
  };
}
