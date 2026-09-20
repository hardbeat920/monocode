/**
 * Placement math for anchored popovers. Kept away from the DOM so the flipping
 * and clamping rules can be tested directly.
 */

export type PopoverSide = "top" | "bottom" | "left" | "right";
export type PopoverAlign = "start" | "center" | "end";

export type AnchorRect = {
  left: number;
  top: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
};

export type Size = { width: number; height: number };
export type ViewportSize = { width: number; height: number };

export type PopoverOptions = {
  /** Preferred side of the anchor; flips only when the space runs out. */
  side?: PopoverSide;
  /** Cross-axis alignment against the anchor. */
  align?: PopoverAlign;
  /** Space between anchor and popover. Negative overlaps, as flyouts like to. */
  gap?: number;
  /** Breathing room kept against every viewport edge. */
  padding?: number;
  /** Fixed width; defaults to the popover's natural width. */
  width?: number;
  /** Height the popover wants before it is worth flipping to the other side. */
  minHeight?: number;
  /** Cap on the height, on top of whatever the viewport allows. */
  maxHeight?: number;
};

export type PopoverPosition = {
  side: PopoverSide;
  left: number;
  /** Set for every side except `top`, which is pinned by its bottom edge. */
  top?: number;
  /** Pinning a top-side popover here keeps it still while its content grows. */
  bottom?: number;
  width: number;
  maxHeight: number;
};

const DEFAULT_GAP = 6;
const DEFAULT_PADDING = 8;

const OPPOSITE: Record<PopoverSide, PopoverSide> = {
  top: "bottom",
  bottom: "top",
  left: "right",
  right: "left",
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

/** Room between the anchor and the viewport edge on one side. */
function roomOn(
  side: PopoverSide,
  anchor: AnchorRect,
  viewport: ViewportSize,
  gap: number,
  padding: number,
): number {
  if (side === "top") return anchor.top - gap - padding;
  if (side === "bottom") return viewport.height - anchor.bottom - gap - padding;
  if (side === "left") return anchor.left - gap - padding;
  return viewport.width - anchor.right - gap - padding;
}

function crossAxisStart(
  align: PopoverAlign,
  start: number,
  end: number,
  length: number,
): number {
  if (align === "start") return start;
  if (align === "end") return end - length;
  return start + (end - start) / 2 - length / 2;
}

export function placePopover(
  anchor: AnchorRect,
  popover: Size,
  viewport: ViewportSize,
  options: PopoverOptions = {},
): PopoverPosition {
  const gap = options.gap ?? DEFAULT_GAP;
  const padding = options.padding ?? DEFAULT_PADDING;
  const align = options.align ?? "start";
  const preferred = options.side ?? "bottom";

  const width = Math.min(
    options.width ?? popover.width,
    Math.max(0, viewport.width - padding * 2),
  );
  const wanted = Math.min(
    options.maxHeight ?? Infinity,
    Math.max(popover.height, options.minHeight ?? 0),
  );

  const vertical = preferred === "top" || preferred === "bottom";
  const needed = vertical ? wanted : width;
  const room = roomOn(preferred, anchor, viewport, gap, padding);
  const flipped = roomOn(OPPOSITE[preferred], anchor, viewport, gap, padding);
  const side = room >= needed || flipped <= room ? preferred : OPPOSITE[preferred];

  if (side === "top" || side === "bottom") {
    const left = clamp(
      crossAxisStart(align, anchor.left, anchor.right, width),
      padding,
      viewport.width - width - padding,
    );
    const available = Math.max(
      roomOn(side, anchor, viewport, gap, padding),
      options.minHeight ?? 0,
    );
    const maxHeight = Math.min(options.maxHeight ?? Infinity, available);
    if (side === "bottom") {
      return { side, left, top: anchor.bottom + gap, width, maxHeight };
    }
    return {
      side,
      left,
      bottom: viewport.height - anchor.top + gap,
      width,
      maxHeight,
    };
  }

  const capped = Math.min(
    options.maxHeight ?? Infinity,
    Math.max(0, viewport.height - padding * 2),
  );
  const height = Math.min(popover.height, capped);
  const top = clamp(
    crossAxisStart(align, anchor.top, anchor.bottom, height),
    padding,
    viewport.height - height - padding,
  );
  const rawLeft =
    side === "right" ? anchor.right + gap : anchor.left - gap - width;
  return {
    side,
    left: clamp(rawLeft, padding, viewport.width - width - padding),
    top,
    width,
    maxHeight: Math.min(capped, viewport.height - top - padding),
  };
}
