import { invoke } from "@tauri-apps/api/core";

export type SystemFont = {
  family: string;
  monospace: boolean;
};

/** Fired on `window` whenever any font setting changes. */
export const FONTS_CHANGE_EVENT = "monocode:fontschange";

const UI_FAMILY_KEY = "monocode.uiFontFamily";
const UI_WEIGHT_KEY = "monocode.uiFontWeight";
const CODE_FAMILY_KEY = "monocode.codeFontFamily";
const CODE_SIZE_KEY = "monocode.codeFontSize";
const CODE_WEIGHT_KEY = "monocode.codeFontWeight";

export const UI_FONT_WEIGHT_DEFAULT = 400;
export const CODE_FONT_SIZE_DEFAULT = 13;
export const CODE_FONT_SIZE_MIN = 11;
export const CODE_FONT_SIZE_MAX = 18;
export const FONT_WEIGHT_MIN = 400;
export const FONT_WEIGHT_MAX = 700;
export const CODE_FONT_WEIGHT_DEFAULT = 400;

const SANS_FALLBACK =
  'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, Ubuntu, Cantarell, "Fira Sans", "Droid Sans", "Helvetica Neue", sans-serif';
const MONO_FALLBACK =
  'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace';

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

export function normalizeFontFamily(value: unknown): string {
  if (typeof value !== "string") return "";
  return value.replace(/[\0-\x1f\x7f]/g, "").trim().slice(0, 120);
}

function escapeFamilyName(family: string): string {
  return family.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

/** `"Picked Font", <fallback stack>`, or the bare fallback when unset. */
export function buildFamilyStack(family: string, fallback: string): string {
  const clean = normalizeFontFamily(family);
  if (!clean) return fallback;
  return `"${escapeFamilyName(clean)}", ${fallback}`;
}

export function uiFontStack(family: string): string {
  return buildFamilyStack(family, SANS_FALLBACK);
}

export function codeFontStack(family: string): string {
  return buildFamilyStack(family, MONO_FALLBACK);
}

export function normalizeFontWeight(value: unknown, fallback: number): number {
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.round(clamp(parsed, FONT_WEIGHT_MIN, FONT_WEIGHT_MAX) / 100) * 100;
}

export function normalizeCodeFontSize(value: unknown): number {
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(parsed)) return CODE_FONT_SIZE_DEFAULT;
  return Math.round(clamp(parsed, CODE_FONT_SIZE_MIN, CODE_FONT_SIZE_MAX));
}

function readFamily(key: string): string {
  try {
    return normalizeFontFamily(localStorage.getItem(key));
  } catch {
    return "";
  }
}

function writeFamily(key: string, value: string) {
  const next = normalizeFontFamily(value);
  try {
    if (next) localStorage.setItem(key, next);
    else localStorage.removeItem(key);
  } catch {
    // private mode / quota
  }
  return next;
}

function readNumber(key: string): number | null {
  try {
    const raw = localStorage.getItem(key);
    if (raw == null) return null;
    const parsed = Number(raw);
    return Number.isFinite(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function notifyFontsChanged() {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new Event(FONTS_CHANGE_EVENT));
}

export function subscribeFonts(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(FONTS_CHANGE_EVENT, onStoreChange);
  return () => window.removeEventListener(FONTS_CHANGE_EVENT, onStoreChange);
}

function setVar(name: string, value: string) {
  try {
    document.documentElement.style.setProperty(name, value);
  } catch {
    // non-DOM (tests)
  }
}

// — Interface knob (sans) —

export function loadUiFontFamily(): string {
  return readFamily(UI_FAMILY_KEY);
}

export function loadUiFontWeight(): number {
  const raw = readNumber(UI_WEIGHT_KEY);
  return normalizeFontWeight(raw ?? UI_FONT_WEIGHT_DEFAULT, UI_FONT_WEIGHT_DEFAULT);
}

export function applyUiFontFamily(family: string): string {
  const next = normalizeFontFamily(family);
  setVar("--font-sans", uiFontStack(next));
  try {
    document.documentElement.classList.toggle(
      "has-custom-ui-font",
      next !== "",
    );
  } catch {
    // non-DOM (tests)
  }
  return next;
}

export function applyUiFontWeight(weight: number): number {
  const next = normalizeFontWeight(weight, UI_FONT_WEIGHT_DEFAULT);
  setVar("--font-sans-weight", String(next));
  return next;
}

export function saveUiFontFamily(family: string): string {
  const next = writeFamily(UI_FAMILY_KEY, family);
  applyUiFontFamily(next);
  notifyFontsChanged();
  return next;
}

export function saveUiFontWeight(weight: number): number {
  const next = normalizeFontWeight(weight, UI_FONT_WEIGHT_DEFAULT);
  try {
    localStorage.setItem(UI_WEIGHT_KEY, String(next));
  } catch {
    // private mode / quota
  }
  applyUiFontWeight(next);
  notifyFontsChanged();
  return next;
}

// — Code knob (mono: editor + terminal + code blocks) —

export function loadCodeFontFamily(): string {
  return readFamily(CODE_FAMILY_KEY);
}

export function loadCodeFontSize(): number {
  return normalizeCodeFontSize(readNumber(CODE_SIZE_KEY) ?? CODE_FONT_SIZE_DEFAULT);
}

export function loadCodeFontWeight(): number {
  const raw = readNumber(CODE_WEIGHT_KEY);
  return normalizeFontWeight(raw ?? CODE_FONT_WEIGHT_DEFAULT, CODE_FONT_WEIGHT_DEFAULT);
}

export function applyCodeFontFamily(family: string): string {
  const next = normalizeFontFamily(family);
  setVar("--font-mono", codeFontStack(next));
  return next;
}

export function applyCodeFontSize(size: number): number {
  const next = normalizeCodeFontSize(size);
  setVar("--font-mono-size", `${next}px`);
  return next;
}

export function applyCodeFontWeight(weight: number): number {
  const next = normalizeFontWeight(weight, CODE_FONT_WEIGHT_DEFAULT);
  setVar("--font-mono-weight", String(next));
  return next;
}

export function saveCodeFontFamily(family: string): string {
  const next = writeFamily(CODE_FAMILY_KEY, family);
  applyCodeFontFamily(next);
  notifyFontsChanged();
  return next;
}

export function saveCodeFontSize(size: number): number {
  const next = normalizeCodeFontSize(size);
  try {
    localStorage.setItem(CODE_SIZE_KEY, String(next));
  } catch {
    // private mode / quota
  }
  applyCodeFontSize(next);
  notifyFontsChanged();
  return next;
}

export function saveCodeFontWeight(weight: number): number {
  const next = normalizeFontWeight(weight, CODE_FONT_WEIGHT_DEFAULT);
  try {
    localStorage.setItem(CODE_WEIGHT_KEY, String(next));
  } catch {
    // private mode / quota
  }
  applyCodeFontWeight(next);
  notifyFontsChanged();
  return next;
}

/** Apply every persisted font setting to `:root`. Called once at boot. */
export function initFonts() {
  applyUiFontFamily(loadUiFontFamily());
  applyUiFontWeight(loadUiFontWeight());
  applyCodeFontFamily(loadCodeFontFamily());
  applyCodeFontSize(loadCodeFontSize());
  applyCodeFontWeight(loadCodeFontWeight());
}

/** Reset both knobs to the system defaults. */
export function resetFontsToDefaults() {
  saveUiFontFamily("");
  saveUiFontWeight(UI_FONT_WEIGHT_DEFAULT);
  saveCodeFontFamily("");
  saveCodeFontSize(CODE_FONT_SIZE_DEFAULT);
  saveCodeFontWeight(CODE_FONT_WEIGHT_DEFAULT);
}

let cachedFonts: Promise<SystemFont[]> | null = null;

/**
 * Enumerate installed families via the Rust backend. Cached in-memory so
 * opening Settings repeatedly does not re-scan. Falls back to [] outside
 * Tauri (plain `vite dev`) or when the scan fails.
 */
export function listSystemFonts(): Promise<SystemFont[]> {
  if (!cachedFonts) {
    cachedFonts = invoke<SystemFont[]>("list_system_fonts")
      .then((fonts) =>
        Array.isArray(fonts)
          ? fonts.filter(
              (font) =>
                font && typeof font.family === "string" && font.family.trim(),
            )
          : [],
      )
      .catch(() => []);
  }
  return cachedFonts;
}
