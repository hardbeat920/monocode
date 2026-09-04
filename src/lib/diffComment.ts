import type { UnifiedLine } from "./unifiedDiff";

export type DiffCommentTarget = {
  path: string;
  line: UnifiedLine;
};

export function diffCommentLocation({ path, line }: DiffCommentTarget): string {
  const number = line.kind === "del" ? line.oldNumber : line.newNumber;
  return number == null ? path : `${path}:${number}`;
}

export function formatDiffComment(
  target: DiffCommentTarget,
  comment: string,
): string {
  const body = comment.replace(/\r\n?/g, "\n").trim();
  if (!body) return "";

  const marker =
    target.line.kind === "add" ? "+" : target.line.kind === "del" ? "-" : " ";
  const deleted = target.line.kind === "del" ? " (deleted line)" : "";
  const location = diffCommentLocation(target).replace(/`/g, "\\`");

  return [
    `Diff comment on \`${location}\`${deleted}:`,
    "",
    `> ${marker}${target.line.text}`,
    "",
    body,
  ].join("\n");
}
