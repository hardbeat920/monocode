import { getCurrentWebview } from "@tauri-apps/api/webview";

const UI_SCALE_KEY = "monocode.uiScale";

export const UI_SCALE_DEFAULT = 1;
export const UI_SCALE_MIN = 0.5;
export const UI_SCALE_MAX = 2;
export const UI_SCALE_STEP = 0.1;

/** Fired on `window` whenever the UI scale changes (detail: number). */
export const UI_SCALE_CHANGE_EVENT = "monocode:uiscalechange";

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function roundStep(value: number) {
  return Math.round(value * 10) / 10;
}

export function normalizeUiScale(value: unknown): number {
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(parsed)) return UI_SCALE_DEFAULT;
  return roundStep(clamp(parsed, UI_SCALE_MIN, UI_SCALE_MAX));
}

export function loadUiScale(): number {
  try {
    const raw = localStorage.getItem(UI_SCALE_KEY);
    if (raw == null) return UI_SCALE_DEFAULT;
    return normalizeUiScale(Number(raw));
  } catch {
    return UI_SCALE_DEFAULT;
  }
}

/** Persist first: `applyUiScale` dispatches synchronously, so listeners that
 * read the store must already see the new value. */
export function saveUiScale(value: number) {
  const next = normalizeUiScale(value);
  try {
    localStorage.setItem(UI_SCALE_KEY, String(next));
  } catch {
    // private mode / quota
  }
  return next;
}

/**
 * App-wide scaling via the native webview zoom (Chromium page zoom /
 * WebView2 zoom), so menu anchors, drag-resize deltas, and terminal layout —
 * all computed from getBoundingClientRect / clientX — stay consistent.
 * Outside Tauri (plain `vite dev` in a browser) it falls back to CSS zoom.
 */
export async function applyUiScale(value: number) {
  const next = normalizeUiScale(value);
  try {
    await getCurrentWebview().setZoom(next);
    document.documentElement.style.removeProperty("zoom");
  } catch {
    // No Tauri runtime (browser dev): fall back to CSS zoom.
    document.documentElement.style.setProperty("zoom", String(next));
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(
      new CustomEvent<number>(UI_SCALE_CHANGE_EVENT, { detail: next }),
    );
  }
  return next;
}

export function zoomInUiScale(current: number = loadUiScale()) {
  return normalizeUiScale(current + UI_SCALE_STEP);
}

export function zoomOutUiScale(current: number = loadUiScale()) {
  return normalizeUiScale(current - UI_SCALE_STEP);
}

/**
 * Ctrl/Cmd + +, -, 0 — the browser-standard zoom keys. `+` arrives as `=`
 * (unshifted) or `+` (shifted) depending on layout; `-` arrives as `-`
 * (unshifted) or `_` (shifted). Numpad keys report via `code`.
 */
export function uiScaleCommand(e: {
  key: string;
  code: string;
}): "zoom-in" | "zoom-out" | "zoom-reset" | null {
  if (e.code === "NumpadAdd" || e.key === "+" || e.key === "=") return "zoom-in";
  if (e.code === "NumpadSubtract" || e.key === "-" || e.key === "_") {
    return "zoom-out";
  }
  if (e.code === "Numpad0" || e.key === "0") return "zoom-reset";
  return null;
}

export function subscribeUiScale(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(UI_SCALE_CHANGE_EVENT, onStoreChange);
  return () => window.removeEventListener(UI_SCALE_CHANGE_EVENT, onStoreChange);
}
