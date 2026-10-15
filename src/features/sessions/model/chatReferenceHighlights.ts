import type { ChatReference } from "./chatReferences";

const HIGHLIGHT = "monocode-chat-reference";
const rangesByOwner = new Map<symbol, Range[]>();

/** A cited range whose text is gone (the message re-rendered) has nothing to mark. */
function liveRange(reference: ChatReference): Range | null {
  const range = reference.range;
  return range && !range.collapsed && range.commonAncestorContainer.isConnected
    ? range
    : null;
}

/** Marks every cited span in the transcript; each pane owns its own set. */
export function paintChatReferenceHighlights(
  owner: symbol,
  references: ChatReference[],
): void {
  const ranges = references.flatMap((reference) => liveRange(reference) ?? []);
  if (ranges.length > 0) rangesByOwner.set(owner, ranges);
  else rangesByOwner.delete(owner);
  if (typeof CSS === "undefined" || !("highlights" in CSS)) return;
  const all = [...rangesByOwner.values()].flat();
  if (all.length > 0) CSS.highlights.set(HIGHLIGHT, new Highlight(...all));
  else CSS.highlights.delete(HIGHLIGHT);
}

/** Removes this pane's marks, leaving other panes' in place. */
export function clearChatReferenceHighlights(owner: symbol): void {
  paintChatReferenceHighlights(owner, []);
}

/** The reference whose marked text sits under the pointer, if any. */
export function chatReferenceAtPoint(
  references: ChatReference[],
  x: number,
  y: number,
): ChatReference | null {
  for (const reference of references) {
    const range = liveRange(reference);
    if (!range) continue;
    for (const rect of Array.from(range.getClientRects())) {
      if (
        x >= rect.left &&
        x <= rect.right &&
        y >= rect.top &&
        y <= rect.bottom
      )
        return reference;
    }
  }
  return null;
}
