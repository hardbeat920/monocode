/** OSC 10/11/12 query payload from a CLI asking for fg/bg/cursor color. */
export function isOscColorQuery(data: string): boolean {
  return data === "?" || data.startsWith("?");
}

/** Reply to a terminal color query using a resolved `#rrggbb` value. */
export function oscColorReply(code: 10 | 11 | 12, hex: string): string {
  const value = hex.replace(/^#/, "");
  if (value.length !== 6) return "";
  const r = value.slice(0, 2);
  const g = value.slice(2, 4);
  const b = value.slice(4, 6);
  return `\x1b]${code};rgb:${r}${r}/${g}${g}/${b}${b}\x1b\\`;
}
