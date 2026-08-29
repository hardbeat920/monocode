export const SELECTABLE_AGENT_RESPONSE_ATTR = "data-selectable-agent-response";

export type TranscriptSelection = {
  text: string;
  rect: DOMRect;
};

export type TranscriptSelectionCandidate = {
  text: string;
  collapsed: boolean;
  anchorResponseId: string | null;
  focusResponseId: string | null;
};

export type RectLike = {
  left: number;
  top: number;
  bottom: number;
  width: number;
};

export type Size = { width: number; height: number };
export type ViewportSize = { width: number; height: number };
export type SelectionMenuPlacement = {
  side: "top" | "bottom";
  top: number;
  left: number;
};

const VIEWPORT_PADDING = 8;
const MENU_OFFSET = 6;

export function validateTranscriptSelection(
  candidate: TranscriptSelectionCandidate,
): string | null {
  const text = candidate.text.trim();
  if (!text || candidate.collapsed) return null;
  if (!candidate.anchorResponseId || !candidate.focusResponseId) return null;
  if (candidate.anchorResponseId !== candidate.focusResponseId) return null;
  return text;
}

export function placeSelectionMenu(
  rect: RectLike,
  menu: Size,
  viewport: ViewportSize,
): SelectionMenuPlacement {
  const fitsAbove = rect.top - MENU_OFFSET - menu.height >= VIEWPORT_PADDING;
  const fitsBelow =
    rect.bottom + MENU_OFFSET + menu.height <=
    viewport.height - VIEWPORT_PADDING;
  const side =
    fitsAbove || (!fitsBelow && rect.top >= viewport.height - rect.bottom)
      ? "top"
      : "bottom";
  const maxLeft = Math.max(
    VIEWPORT_PADDING,
    viewport.width - menu.width - VIEWPORT_PADDING,
  );
  const left = Math.min(
    maxLeft,
    Math.max(VIEWPORT_PADDING, rect.left + rect.width / 2 - menu.width / 2),
  );
  const rawTop =
    side === "top"
      ? rect.top - MENU_OFFSET - menu.height
      : rect.bottom + MENU_OFFSET;
  const maxTop = Math.max(
    VIEWPORT_PADDING,
    viewport.height - menu.height - VIEWPORT_PADDING,
  );
  const top = Math.min(maxTop, Math.max(VIEWPORT_PADDING, rawTop));
  return { side, left, top };
}

const FLYOUT_OVERLAP = 4;

/** Nested menu to the right of a parent, flipping left when space is tight. */
export function placeFlyoutMenu(
  parent: { left: number; width: number },
  rowTop: number,
  menu: Size,
  viewport: ViewportSize,
): { side: "right" | "left"; left: number; top: number } {
  const parentRight = parent.left + parent.width;
  const rightLeft = parentRight - FLYOUT_OVERLAP;
  const leftLeft = parent.left - menu.width + FLYOUT_OVERLAP;
  const fitsRight = rightLeft + menu.width <= viewport.width - VIEWPORT_PADDING;
  const side: "right" | "left" =
    fitsRight || leftLeft < VIEWPORT_PADDING ? "right" : "left";
  const rawLeft = side === "right" ? rightLeft : leftLeft;
  const maxLeft = Math.max(
    VIEWPORT_PADDING,
    viewport.width - menu.width - VIEWPORT_PADDING,
  );
  const left = Math.min(maxLeft, Math.max(VIEWPORT_PADDING, rawLeft));
  const maxTop = Math.max(
    VIEWPORT_PADDING,
    viewport.height - menu.height - VIEWPORT_PADDING,
  );
  const top = Math.min(maxTop, Math.max(VIEWPORT_PADDING, rowTop));
  return { side, left, top };
}
