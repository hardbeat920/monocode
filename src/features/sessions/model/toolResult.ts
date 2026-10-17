import type { Block } from "./session";
import { needsApproval, toolCallState } from "./transcriptActivity";

/** Preview output is explicit; detail may instead be a retained request summary. */
export function successfulToolOutput(block: Block): string | undefined {
  // A missing status can be an old pending call with a retained input preview.
  // Require a reported completion before interpreting any field as output.
  const status = block.tool?.status?.toLowerCase();
  if (
    needsApproval(block) ||
    (status !== "completed" && status !== "success") ||
    toolCallState(block) !== "accepted"
  ) return;
  const preview = block.tool?.preview?.output;
  if (preview?.trim()) return preview;
  const detail = block.tool?.detail;
  return block.tool?.detailIsOutput && detail?.trim() ? detail : undefined;
}
