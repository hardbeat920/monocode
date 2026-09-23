/**
 * Persisted layout of the code view's outline panel: whether it is open, how
 * wide the docked column is, and — when detached — where the floating card
 * sits. Mirrors the settings store pattern (localStorage + change event) so
 * every editor pane stays in sync without threading props through the tree.
 */

export type OutlinePosition = { x: number; y: number };

export type OutlineView = {
  open: boolean;
  width: number;
  detached: boolean;
  /** Floating card's top-left in viewport coordinates; null until first detach. */
  position: OutlinePosition | null;
};

export const OUTLINE_WIDTH_MIN = 160;
export const OUTLINE_WIDTH_MAX = 520;
export const OUTLINE_WIDTH_DEFAULT = 240;

export const OUTLINE_VIEW_DEFAULT: OutlineView = {
  open: false,
  width: OUTLINE_WIDTH_DEFAULT,
  detached: false,
  position: null,
};

const OUTLINE_VIEW_KEY = "monocode.outlineView";

/** Fired on `window` when any part of the outline layout changes. */
export const OUTLINE_VIEW_CHANGE_EVENT = "monocode:outline-view-change";

function clampWidth(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return OUTLINE_WIDTH_DEFAULT;
  }
  return Math.min(
    OUTLINE_WIDTH_MAX,
    Math.max(OUTLINE_WIDTH_MIN, Math.round(value)),
  );
}

function readPosition(value: unknown): OutlinePosition | null {
  if (!value || typeof value !== "object") return null;
  const { x, y } = value as Record<string, unknown>;
  if (typeof x !== "number" || typeof y !== "number") return null;
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  return { x, y };
}

function normalize(value: unknown): OutlineView {
  if (!value || typeof value !== "object") return OUTLINE_VIEW_DEFAULT;
  const raw = value as Record<string, unknown>;
  return {
    open: raw.open === true,
    width: clampWidth(raw.width),
    detached: raw.detached === true,
    position: readPosition(raw.position),
  };
}

let cached: OutlineView | null = null;
let cachedRaw: string | null = null;

function safeParse(raw: string): unknown {
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

export function loadOutlineView(): OutlineView {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(OUTLINE_VIEW_KEY);
  } catch {
    raw = null;
  }
  if (cached && cachedRaw === raw) return cached;
  cachedRaw = raw;
  cached = raw ? normalize(safeParse(raw)) : OUTLINE_VIEW_DEFAULT;
  return cached;
}

export function saveOutlineView(view: OutlineView) {
  const next = normalize(view);
  let serialized: string | null = null;
  try {
    serialized = JSON.stringify(next);
    localStorage.setItem(OUTLINE_VIEW_KEY, serialized);
  } catch {
    // private mode / quota
  }
  cached = next;
  cachedRaw = serialized;
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<OutlineView>(OUTLINE_VIEW_CHANGE_EVENT, { detail: next }),
  );
}

export function subscribeOutlineView(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(OUTLINE_VIEW_CHANGE_EVENT, onStoreChange);
  return () =>
    window.removeEventListener(OUTLINE_VIEW_CHANGE_EVENT, onStoreChange);
}
