import type { Block } from "./session";

/** A vertical span in viewport coordinates. */
export type OutlineBand = { top: number; bottom: number };

export type OutlineAnchor = OutlineBand & { id: string };

export const NEAR_END_PX = 16;

export function promptBlocks(blocks: Block[]): Block[] {
  return blocks.filter((block) => block.role === "user");
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
