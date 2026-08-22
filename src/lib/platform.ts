export const IS_MAC =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad/.test(navigator.platform);

export const MOD = IS_MAC ? "⌘" : "Ctrl+";
export const ALT = IS_MAC ? "⌥" : "Alt+";
export const SHIFT = IS_MAC ? "⇧" : "Shift+";
