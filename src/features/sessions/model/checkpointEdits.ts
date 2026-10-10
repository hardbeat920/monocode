import { isEditTool } from "../../../integrations/harness/core/preview";
import { CONTINUE_PROMPT } from "./inFlight";
import type { Block } from "./session";

/** Successful edits from the latest turn and its interrupted continuations. */
export function sessionEditPaths(blocks: readonly Block[]): string[] {
  let start = 0;
  for (let index = blocks.length - 1; index >= 0; index--) {
    const block = blocks[index];
    if (
      block.role !== "user" ||
      block.draft ||
      (block.sentAt != null && block.startedAt == null)
    )
      continue;
    start = index;
    if (block.text.trim() !== CONTINUE_PROMPT) break;
  }
  const paths = new Set<string>();
  for (let index = start; index < blocks.length; index++) {
    const block = blocks[index];
    const tool = block.role === "tool" ? block.tool : undefined;
    if (
      !tool ||
      !["completed", "success"].includes(tool.status ?? "") ||
      !isEditTool(tool.kind, tool.title, tool.preview)
    )
      continue;
    for (const path of tool.paths ?? []) if (path) paths.add(path);
    if (tool.preview?.path) paths.add(tool.preview.path);
  }
  return [...paths];
}
