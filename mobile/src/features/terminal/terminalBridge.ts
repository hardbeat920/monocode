/** What the terminal page tells the app. */
export type PageMessage =
  | { t: "ready" }
  /** Typing, as base64 bytes. */
  | { t: "input"; data: string }
  | { t: "size"; cols: number; rows: number }
  /** A full-screen program (an editor, a pager) took the screen, or let it go. */
  | { t: "full"; on: boolean };

/** The most typing one message may carry, in base64 characters. */
const MAX_INPUT = 96 * 1024;
const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;

const whole = (value: unknown, min: number, max: number): value is number =>
  typeof value === "number" &&
  Number.isInteger(value) &&
  value >= min &&
  value <= max;

/**
 * A message from the page, or null when it is not one. The page shows a remote
 * shell's output, so what it sends is checked before anything acts on it.
 */
export function parsePageMessage(raw: unknown): PageMessage | null {
  if (typeof raw !== "string" || raw.length > MAX_INPUT + 64) return null;
  let message: unknown;
  try {
    message = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof message !== "object" || message === null) return null;
  const { t, data, cols, rows, on } = message as Record<string, unknown>;
  if (t === "ready") return { t };
  if (t === "input")
    return typeof data === "string" &&
      data.length <= MAX_INPUT &&
      BASE64.test(data)
      ? { t, data }
      : null;
  if (t === "full") return typeof on === "boolean" ? { t, on } : null;
  if (t === "size")
    return whole(cols, 2, 400) && whole(rows, 1, 200)
      ? { t, cols, rows }
      : null;
  return null;
}

/** The script that draws output on the page; `reset` clears the screen first. */
export function writeScript(data: string, reset: boolean): string {
  return `window.__mono.write(${JSON.stringify(data)},${reset});`;
}

export const FOCUS_SCRIPT = "window.__mono.focus();";
/** The terminal's colours: the app's, with the desktop's ANSI palette for the scheme. */
export type TerminalLook = {
  light: boolean;
  background: string;
  foreground: string;
  cursor: string;
};

/** Run before the page loads, so it draws in the right scheme from the start. */
export const lookPreload = (look: TerminalLook) =>
  `window.__monoLook=${JSON.stringify(look)};true;`;

export const lookScript = (look: TerminalLook) =>
  `window.__mono.look(${JSON.stringify(look)});`;

/** How much of the page's bottom the key bar covers, in points. */
export const insetScript = (px: number) =>
  `window.__mono.inset(${Math.max(0, Math.round(px))});`;

export const BLUR_SCRIPT = "window.__mono.blur();";

/** The script that pastes base64 UTF-8 text, as the terminal pastes. */
export function pasteScript(data: string): string {
  return `window.__mono.paste(${JSON.stringify(data)});`;
}

/** Scripts handed to the WebView must end in a value, or iOS logs a warning. */
export const run = (scripts: string[]) => `${scripts.join("")}true;`;
