import { isTauriRuntime } from "../bridge/isTauri";

export const IS_MAC =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad/.test(navigator.platform);

/** Native macOS window chrome (traffic lights). False in browser remote sessions. */
export const MAC_WINDOW_CHROME = IS_MAC && isTauriRuntime();

export const MOD = IS_MAC ? "⌘" : "Ctrl+";
export const ALT = IS_MAC ? "⌥" : "Alt+";
export const SHIFT = IS_MAC ? "⇧" : "Shift+";
