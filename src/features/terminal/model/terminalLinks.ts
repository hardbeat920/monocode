import type { IBufferRange, Terminal } from "@xterm/xterm";

/**
 * The text an OSC 8 hyperlink shows on screen. xterm reports one row per
 * range, with 1-based, inclusive columns.
 */
export function oscLinkLabel(term: Terminal, range: IBufferRange): string {
  const line = term.buffer.active.getLine(range.start.y - 1);
  return line?.translateToString(true, range.start.x - 1, range.end.x) ?? "";
}

/**
 * An OSC 8 hyperlink can show one address and open another. Ask before opening
 * one unless it shows exactly where it goes.
 */
export function oscLinkNeedsConfirm(label: string, uri: string): boolean {
  return label.trim() !== uri;
}
