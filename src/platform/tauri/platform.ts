export const IS_MAC =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad/.test(navigator.platform);

export const IS_WIN =
  typeof navigator !== "undefined" && /Win/i.test(navigator.platform);

/** Native desktop blur: macOS vibrancy and Windows acrylic. Linux stays opaque. */
export const HAS_NATIVE_GLASS = IS_MAC || IS_WIN;

export const MOD = IS_MAC ? "⌘" : "Ctrl+";
export const ALT = IS_MAC ? "⌥" : "Alt+";
export const SHIFT = IS_MAC ? "⇧" : "Shift+";
