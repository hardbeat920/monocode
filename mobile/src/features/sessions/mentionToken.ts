/** Desktop fileMentions token grammar. Keep parity covered by mentionToken tests. */
export type MentionToken = { start: number; end: number; query: string };
const MAX_QUERY = 120;
function isSpace(ch: string) {
  return /[ \n\t\r]/.test(ch);
}
function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}
function isMarkdownBlockquotePosition(text: string, position: number) {
  const index = Math.max(0, Math.min(position, text.length));
  const lineStart = text.lastIndexOf("\n", Math.max(0, index - 1)) + 1;
  return /^ {0,3}>/.test(text.slice(lineStart, index));
}

/** Mention token that contains `cursor`, if the user is typing `@file`. */
export function mentionTokenAt(
  text: string,
  cursor: number,
): MentionToken | null {
  const i = clamp(cursor, 0, text.length);
  let start = i;
  while (start > 0 && !isSpace(text[start - 1]!)) start -= 1;
  if (text[start] !== "@") return null;
  if (isMarkdownBlockquotePosition(text, start)) return null;

  let end = start + 1;
  while (end < text.length && !isSpace(text[end]!)) end += 1;

  const typed = text.slice(start + 1, i);
  if (typed.includes("@") || typed.length > MAX_QUERY) return null;

  return { start, end, query: typed };
}

export function replaceMentionToken(
  text: string,
  token: MentionToken,
  label: string,
): string {
  const rest = text.slice(token.end);
  const spacer = rest.startsWith(" ") ? "" : " ";
  return `${text.slice(0, token.start)}@${label}${spacer}${rest}`;
}
