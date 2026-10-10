import { useSyncExternalStore } from "react";

const TERMINAL_FONT_KEY = "monocode.terminalFontFamily";

/** Fired on `window` whenever the terminal font changes (detail: string). */
export const TERMINAL_FONT_CHANGE_EVENT = "monocode:terminalfontchange";

const DEFAULT_MONO_STACK = [
  "ui-monospace",
  "SFMono-Regular",
  "Menlo",
  "Monaco",
  "Consolas",
  '"Liberation Mono"',
  '"Courier New"',
];

/**
 * Patched Nerd Font families tried after the regular mono stack, so prompt
 * themes (Powerlevel10k, Starship, oh-my-posh…) get their Private Use Area
 * icons from whichever one is installed without changing ASCII rendering.
 */
export const NERD_FONT_FALLBACKS = [
  '"Symbols Nerd Font Mono"',
  '"Symbols Nerd Font"',
  '"MesloLGS NF"',
  '"MesloLGS Nerd Font Mono"',
  '"JetBrainsMono Nerd Font Mono"',
  '"JetBrainsMono Nerd Font"',
  '"FiraCode Nerd Font Mono"',
  '"FiraCode Nerd Font"',
  '"Hack Nerd Font Mono"',
  '"Hack Nerd Font"',
  '"CaskaydiaCove Nerd Font Mono"',
  '"CaskaydiaCove Nerd Font"',
];

const GENERIC_FAMILIES = new Set([
  "monospace",
  "serif",
  "sans-serif",
  "system-ui",
  "cursive",
  "fantasy",
]);

/** Splits a CSS `font-family` list on top-level commas. */
export function splitFontFamilies(value: string): string[] {
  const out: string[] = [];
  let current = "";
  let quote: string | null = null;
  for (const ch of value) {
    if (quote) {
      current += ch;
      if (ch === quote) quote = null;
    } else if (ch === '"' || ch === "'") {
      current += ch;
      quote = ch;
    } else if (ch === ",") {
      out.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  out.push(current);
  return out.map((family) => family.trim()).filter(Boolean);
}

/** Quotes a bare family name containing spaces so `Hack Nerd Font` works. */
function normalizeFamily(family: string): string {
  if (/^["'].*["']$/.test(family)) return family;
  if (GENERIC_FAMILIES.has(family.toLowerCase())) return family;
  return /[^\w-]/.test(family) ? `"${family.replace(/"/g, "")}"` : family;
}

function familyKey(family: string): string {
  return family.replace(/^["']|["']$/g, "").toLowerCase();
}

/**
 * Builds the xterm `fontFamily`: the user's preferred font first, then the
 * app mono stack, then Nerd Font fallbacks, ending with generic `monospace`.
 */
export function terminalFontFamily(custom: string, baseStack?: string): string {
  const base = baseStack?.trim()
    ? splitFontFamilies(baseStack)
    : DEFAULT_MONO_STACK;
  const families = [
    ...splitFontFamilies(custom).map(normalizeFamily),
    ...base.filter((family) => familyKey(family) !== "monospace"),
    ...NERD_FONT_FALLBACKS,
    "monospace",
  ];
  const seen = new Set<string>();
  return families
    .filter((family) => {
      const key = familyKey(family);
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .join(", ");
}

let unsaved: string | null = null;

export function loadTerminalFont(): string {
  if (unsaved != null) return unsaved;
  try {
    return localStorage.getItem(TERMINAL_FONT_KEY) ?? "";
  } catch {
    return "";
  }
}

export function saveTerminalFont(value: string) {
  const next = value.trim();
  try {
    if (next) localStorage.setItem(TERMINAL_FONT_KEY, next);
    else localStorage.removeItem(TERMINAL_FONT_KEY);
    unsaved = null;
  } catch {
    // private mode / quota: keep it for this window only
    unsaved = next;
  }
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<string>(TERMINAL_FONT_CHANGE_EVENT, { detail: next }),
  );
}

export function subscribeTerminalFont(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  // Other windows write the same localStorage; `storage` reports their saves.
  const onStorage = (storage: StorageEvent) => {
    if (storage.key !== TERMINAL_FONT_KEY && storage.key !== null) return;
    unsaved = null;
    onStoreChange();
  };
  window.addEventListener(TERMINAL_FONT_CHANGE_EVENT, onStoreChange);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(TERMINAL_FONT_CHANGE_EVENT, onStoreChange);
    window.removeEventListener("storage", onStorage);
  };
}

export function useTerminalFont(): string {
  return useSyncExternalStore(subscribeTerminalFont, loadTerminalFont, () => "");
}
