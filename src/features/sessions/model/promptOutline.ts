import type { Block } from "./session";

/** A vertical span in viewport coordinates. */
export type OutlineBand = { top: number; bottom: number };

export type OutlineAnchor = OutlineBand & { id: string };

export const NEAR_END_PX = 16;

export function promptBlocks(blocks: Block[]): Block[] {
  return blocks.filter((block) => block.role === "user" && !block.internal);
}

/**
 * Selects the topmost prompt inside the viewport. With no prompt inside,
 * selects the last prompt above the viewport, else the first prompt. Near the
 * end of the transcript, selects the last prompt: the prompts on the final
 * screen can not reach the top.
 */
export function activePromptId(
  viewport: OutlineBand,
  anchors: OutlineAnchor[],
  distanceToEnd = Number.POSITIVE_INFINITY,
): string | null {
  if (anchors.length === 0) return null;
  if (distanceToEnd <= NEAR_END_PX) return anchors[anchors.length - 1].id;
  const inside = anchors.find(
    (anchor) => anchor.bottom > viewport.top && anchor.top < viewport.bottom,
  );
  if (inside) return inside.id;
  let above: OutlineAnchor | undefined;
  for (const anchor of anchors) {
    if (anchor.bottom <= viewport.top) above = anchor;
  }
  return (above ?? anchors[0]).id;
}

/** Selects at most `max` prompts. The window slides to keep the active prompt inside. It prefers the newest prompts. */
export function barWindow(
  count: number,
  activeIndex: number | null,
  max: number,
): { start: number; end: number } {
  if (count <= max) return { start: 0, end: count };
  const newest = count - max;
  const start =
    activeIndex == null ? newest : Math.max(0, Math.min(newest, activeIndex));
  return { start, end: start + max };
}

export function promptLabel(block: Block): string {
  const card = block.secondOpinion;
  const textShown = !card || card.kind === "handoff";
  const text = textShown ? firstLine(block.text) : "";
  if (text) return text;
  if (card) {
    if (card.kind === "handoff") return "Handoff";
    const request = firstLine(card.request ?? "");
    return request ? `Second opinion: ${request}` : "Second opinion";
  }
  if (block.noteCard?.title) return block.noteCard.title;
  const files = block.attachments ?? [];
  if (files.length > 0) {
    const [first] = files;
    return files.length > 1 ? `${first.name} +${files.length - 1}` : first.name;
  }
  return "Empty message";
}

function firstLine(text: string): string {
  const line = text
    .split(/\r?\n/)
    .map((part) => part.trim())
    .find(Boolean);
  return (line ?? "").replace(/\s+/g, " ");
}

/** How far the hover ripple reaches, in bars on each side. */
export const RIPPLE_SPAN = 2;

/** Dock-style magnification: 1 on the hovered bar, tapering to 0 past the ripple span. */
export function barLift(index: number, hoverIndex: number | null): number {
  if (hoverIndex == null || hoverIndex < 0) return 0;
  const distance = Math.abs(index - hoverIndex);
  if (distance > RIPPLE_SPAN) return 0;
  return (RIPPLE_SPAN + 1 - distance) / (RIPPLE_SPAN + 1);
}

/** The prompt and the head of its reply, shown while a bar is hovered. */
export type PromptPreview = {
  title: string;
  reply?: string;
  detail?: string;
};

const REPLY_SCAN_CHARS = 2000;

export function promptPreview(
  blocks: Block[],
  promptId: string,
): PromptPreview | null {
  const index = blocks.findIndex((block) => block.id === promptId);
  if (index < 0) return null;
  let reply: string[] = [];
  for (let i = index + 1; i < blocks.length; i += 1) {
    const block = blocks[i];
    if (block.role === "user") break;
    if (block.role !== "assistant") continue;
    reply = previewLines(block.text, 2);
    if (reply.length > 0) break;
  }
  return {
    title: promptLabel(blocks[index]),
    reply: reply[0],
    detail: reply[1],
  };
}

/** The first `max` prose lines of a reply. Markers, fenced code and rules drop out. */
export function previewLines(text: string, max: number): string[] {
  const lines: string[] = [];
  let fenced = false;
  for (const raw of text.slice(0, REPLY_SCAN_CHARS).split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith("```")) {
      fenced = !fenced;
      continue;
    }
    if (fenced) continue;
    const plain = line
      .replace(/^(?:[#>]+|[-*+]|\d+[.)])\s+/, "")
      .replace(/\*\*|`/g, "")
      .replace(/\s+/g, " ")
      .trim();
    // Rules, table separators and lone punctuation read as noise in a preview.
    if (!/[\p{L}\p{N}]/u.test(plain)) continue;
    lines.push(plain);
    if (lines.length === max) break;
  }
  return lines;
}
