/** ATX heading at the start of a source line (`#` through `######`). */
export function isAtxHeadingLine(line: string): boolean {
  return /^\s{0,3}#{1,6}(?:\s|$)/.test(line);
}
