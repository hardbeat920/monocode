export type FontKind = "ui" | "code";

const KEYS = { ui: "monocode.uiFontFamily", code: "monocode.codeFontFamily" };
const TOKENS = { ui: "--font-sans", code: "--font-mono" };
export const FONTS_CHANGE_EVENT = "monocode:fontschange";

export function loadFontFamily(kind: FontKind): string {
  try {
    return localStorage.getItem(KEYS[kind]) ?? "";
  } catch {
    return "";
  }
}

/** Family names are literals, never CSS declarations or comma-separated lists. */
export function fontFamilyCss(kind: FontKind, family: string): string {
  const fallback = `var(${TOKENS[kind]}-default)`;
  if (!family) return fallback;
  const quoted = family.replace(/["\\\n\r\f]/g, (char) =>
    char === '"' || char === "\\"
      ? `\\${char}`
      : `\\${char.charCodeAt(0).toString(16)} `,
  );
  return `"${quoted}", ${fallback}`;
}

function applyFont(kind: FontKind, family: string) {
  if (family)
    document.documentElement.style.setProperty(
      TOKENS[kind],
      fontFamilyCss(kind, family),
    );
  else document.documentElement.style.removeProperty(TOKENS[kind]);
}

export function applyFonts() {
  applyFont("ui", loadFontFamily("ui"));
  applyFont("code", loadFontFamily("code"));
  window.dispatchEvent(new Event(FONTS_CHANGE_EVENT));
}

export function saveFontFamily(kind: FontKind, family: string) {
  if (family) localStorage.setItem(KEYS[kind], family);
  else localStorage.removeItem(KEYS[kind]);
  applyFonts();
}

export function subscribeFonts(listener: () => void) {
  window.addEventListener(FONTS_CHANGE_EVENT, listener);
  return () => window.removeEventListener(FONTS_CHANGE_EVENT, listener);
}

let initialized = false;
export function initFonts() {
  applyFonts();
  if (initialized) return;
  initialized = true;
  window.addEventListener("storage", (event) => {
    if (event.key === null || Object.values(KEYS).includes(event.key))
      applyFonts();
  });
}
