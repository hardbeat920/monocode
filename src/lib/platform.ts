const USER_AGENT =
  typeof navigator !== "undefined" ? navigator.userAgent : "";

/** True on iPad, including iPads masquerading as MacIntel in Safari. */
export const IS_IPAD =
  /iPad/.test(USER_AGENT) ||
  (/Macintosh/.test(USER_AGENT) &&
    typeof navigator !== "undefined" &&
    (navigator as Navigator & { maxTouchPoints?: number }).maxTouchPoints !=
      null &&
    (navigator as Navigator & { maxTouchPoints?: number }).maxTouchPoints! >
      1);

/** Apple desktop or iPad/iPhone — keyboard glyphs, hide Windows window buttons. */
export const IS_MAC =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad/.test(navigator.platform);

/**
 * macOS app window only. iPad WKWebView is opaque; the Mac glass class
 * (`background: transparent`) flashes through to the system surface.
 */
export const IS_MACOS = IS_MAC && !IS_IPAD;

/** True when the primary pointer is touch (companion touch-layout switch). */
export const IS_TOUCH =
  typeof window !== "undefined" &&
  (("ontouchstart" in window && !IS_MAC) || IS_IPAD);

export const MOD = IS_MAC ? "⌘" : "Ctrl+";
export const ALT = IS_MAC ? "⌥" : "Alt+";
export const SHIFT = IS_MAC ? "⇧" : "Shift+";

/** Keyboard glyphs in chrome. iPad is IS_MAC but has no modifier keys. */
export const SHOW_KEY_SHORTCUTS = IS_MACOS;
