export type ReaderAnchor = { id: string; character: number; top: number };

/** Bookmark a visible glyph, rather than a scroll offset or a node boundary. */
export function captureReaderAnchor(): ReaderAnchor {
  const scroller = document.querySelector<HTMLElement>(".agent-transcript")!;
  const viewport = scroller.getBoundingClientRect();
  for (const offsetY of [32, 48, 64]) {
    for (const fraction of [0.25, 0.5, 0.75]) {
      const range = document.caretRangeFromPoint(
        viewport.left + viewport.width * fraction,
        viewport.top + offsetY,
      );
      if (!range || range.startContainer.nodeType !== Node.TEXT_NODE) continue;
      const node = range.startContainer as Text;
      const message = node.parentElement?.closest<HTMLElement>(
        '[data-chat-message-role="assistant"]',
      );
      if (!message || !scroller.contains(message) || !node.length) continue;
      const offset = Math.min(range.startOffset, node.length - 1);
      if (/\s/.test(node.data[offset])) continue;
      range.setStart(node, offset);
      range.setEnd(node, offset + 1);
      const rect = range.getBoundingClientRect();
      if (!rect.width || !rect.height) continue;
      const prefix = document.createRange();
      prefix.selectNodeContents(message);
      prefix.setEnd(node, offset);
      return {
        id: message.dataset.chatMessage!,
        character: prefix.toString().length,
        top: rect.top - viewport.top,
      };
    }
  }
  throw new Error("No visible reply character to bookmark");
}

export function readerAnchorTop(anchor: ReaderAnchor): number {
  const scroller = document.querySelector<HTMLElement>(".agent-transcript")!;
  const message = scroller.querySelector(
    `[data-chat-message="${CSS.escape(anchor.id)}"]`,
  );
  if (!message) return Infinity;
  const walker = document.createTreeWalker(message, NodeFilter.SHOW_TEXT);
  let remaining = anchor.character;
  let node: Text | null;
  while ((node = walker.nextNode() as Text | null)) {
    if (remaining < node.length) {
      const range = document.createRange();
      range.setStart(node, remaining);
      range.setEnd(node, remaining + 1);
      return (
        range.getBoundingClientRect().top - scroller.getBoundingClientRect().top
      );
    }
    remaining -= node.length;
  }
  return Infinity;
}
