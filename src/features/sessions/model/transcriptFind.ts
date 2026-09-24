import type { Block } from "./session";

export function transcriptBlockText(block: Block): string {
  const parts: string[] = [];
  if (block.text.trim()) parts.push(block.text);
  if (block.tool?.title) parts.push(block.tool.title);
  if (block.tool?.detail) parts.push(block.tool.detail);
  if (block.tool?.preview?.query) parts.push(block.tool.preview.query);
  if (block.tool?.preview?.path) parts.push(block.tool.preview.path);
  if (block.tool?.preview?.output) parts.push(block.tool.preview.output);
  if (block.tool?.preview?.title) parts.push(block.tool.preview.title);
  return parts.join("\n");
}

export function findTranscriptBlocks(blocks: Block[], query: string): string[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  return blocks
    .filter(
      (block) =>
        (block.role === "user" ||
          block.role === "assistant" ||
          block.role === "tool" ||
          block.role === "tasks" ||
          block.role === "plan") &&
        transcriptBlockText(block).toLowerCase().includes(needle),
    )
    .map((block) => block.id);
}
