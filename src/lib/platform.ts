export const IS_MAC =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad/.test(navigator.platform);

export const IS_WIN =
  typeof navigator !== "undefined" && /Win/i.test(navigator.platform);

/**
 * Has a glass-capable surface. macOS/Windows use OS-level vibrancy/acrylic
 * (compositor-side blur). Linux relies on the transparent Tauri window plus
 * CSS `backdrop-filter`; if the compositor can't blur, the CSS fallback
 * (translucent tint + border + shadow) still keeps the panel from looking flat.
 */
export const HAS_NATIVE_GLASS = true;

export const MOD = IS_MAC ? "⌘" : "Ctrl+";
export const ALT = IS_MAC ? "⌥" : "Alt+";
export const SHIFT = IS_MAC ? "⇧" : "Shift+";
