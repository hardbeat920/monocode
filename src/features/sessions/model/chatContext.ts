/**
 * Context attached with "Add to chat". The composer shows each item as a chip
 * instead of pasting markdown into the draft. On send, the items follow the
 * message text in one <attached_context> block. The agent reads that block,
 * and the transcript parses it back into the same chips.
 */
export type ChatContextItem =
  | { kind: "quote"; text: string }
  | { kind: "code"; path: string; startLine: number; endLine: number }
  | {
      kind: "comment";
      path: string;
      /** New-file line number. A removed line uses its old-file number. */
      line?: number;
      change: DiffLineChange;
      code: string;
      comment: string;
    };

export type DiffLineChange = "added" | "removed" | "unchanged";

export type ChatContextMessage = {
  text: string;
  items: ChatContextItem[];
};

const OPEN = "<attached_context>";
const CLOSE = "</attached_context>";
const CHANGES: readonly DiffLineChange[] = ["added", "removed", "unchanged"];

// Item bodies are user text, so a reserved tag inside one gets one extra
// backslash after its "<". Parsing removes exactly one, so any text survives
// the round trip and only real delimiters look like delimiters.
const RESERVED_TAG =
  /<(\\*)(\/?)(attached_context|quoted_text|code_selection|review_comment)\b/g;
const ESCAPED_TAG =
  /<\\(\\*)(\/?)(attached_context|quoted_text|code_selection|review_comment)\b/g;
const ITEM =
  /\n<(quoted_text|code_selection|review_comment)((?: [a-z_]+="[^"]*")*)(?: \/>|>\n([\s\S]*?)\n<\/\1>)/y;
const ATTRIBUTE = / ([a-z_]+)="([^"]*)"/g;
const ENTITIES: Record<string, string> = {
  amp: "&",
  quot: '"',
  lt: "<",
  gt: ">",
  "#10": "\n",
};

/** A quote chip for selected transcript text, or null when nothing is selected. */
export function quoteContext(text: string): ChatContextItem | null {
  const value = normalize(text).trim();
  return value ? { kind: "quote", text: value } : null;
}

/** Appends the items to a message. Without items, the message is unchanged. */
export function composeChatContext(
  text: string,
  items: readonly ChatContextItem[],
): string {
  if (items.length === 0) return text;
  const block = [OPEN, ...items.map(formatItem), CLOSE].join("\n");
  return text.trim() ? `${text}\n\n${block}` : block;
}

/**
 * Splits a message into its typed text and attached items. A message that
 * does not end in a well-formed block comes back unchanged with no items.
 */
export function splitChatContext(message: string): ChatContextMessage {
  const plain = { text: message, items: [] };
  const trimmed = message.replace(/\s+$/, "");
  if (!trimmed.endsWith(CLOSE)) return plain;
  const start = trimmed.lastIndexOf(OPEN);
  if (start < 0 || (start > 0 && trimmed[start - 1] !== "\n")) return plain;

  const items = parseItems(
    trimmed.slice(start + OPEN.length, trimmed.length - CLOSE.length),
  );
  if (!items) return plain;

  const before = trimmed.slice(0, start);
  const text = before.endsWith("\n\n")
    ? before.slice(0, -2)
    : before.slice(0, -1);
  return { text, items };
}

/** Adds an item unless the same one is already attached. */
export function addChatContext(
  items: readonly ChatContextItem[],
  item: ChatContextItem,
): ChatContextItem[] {
  const key = chatContextKey(item);
  return items.some((entry) => chatContextKey(entry) === key)
    ? [...items]
    : [...items, item];
}

/** Identifies an item by its content. Equal items share a key. */
export function chatContextKey(item: ChatContextItem): string {
  return JSON.stringify(item);
}

/** "12" for one line, "12-40" for a range. */
export function lineRange(startLine: number, endLine: number): string {
  return endLine > startLine ? `${startLine}-${endLine}` : `${startLine}`;
}

export function contextFileName(path: string): string {
  return path.split("/").filter(Boolean).pop() ?? path;
}

/** One line naming the item, for places too narrow for a chip. */
export function chatContextLabel(item: ChatContextItem): string {
  if (item.kind === "quote") return contextExcerpt(item.text);
  if (item.kind === "code") {
    return `${contextFileName(item.path)}:${lineRange(item.startLine, item.endLine)}`;
  }
  const location =
    item.line != null
      ? `${contextFileName(item.path)}:${item.line}`
      : contextFileName(item.path);
  return `${location} ${contextExcerpt(item.comment)}`;
}

/** Label for a list of items: the first one, and how many follow it. */
export function chatContextSummary(items: readonly ChatContextItem[]): string {
  const [first] = items;
  if (!first) return "";
  const label = chatContextLabel(first);
  return items.length > 1 ? `${label} +${items.length - 1}` : label;
}

function formatItem(item: ChatContextItem): string {
  if (item.kind === "quote") {
    return ["<quoted_text>", quoteLines(item.text), "</quoted_text>"].join(
      "\n",
    );
  }
  if (item.kind === "code") {
    const lines = lineRange(item.startLine, item.endLine);
    return `<code_selection path="${escapeAttribute(item.path)}" lines="${lines}" />`;
  }
  const line = item.line != null ? ` line="${item.line}"` : "";
  return [
    `<review_comment path="${escapeAttribute(item.path)}"${line} change="${item.change}">`,
    quoteLines(item.code),
    "",
    escapeBody(item.comment),
    "</review_comment>",
  ].join("\n");
}

function parseItems(body: string): ChatContextItem[] | null {
  const items: ChatContextItem[] = [];
  let end = 0;
  ITEM.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = ITEM.exec(body))) {
    const item = parseItem(match[1]!, attributes(match[2] ?? ""), match[3]);
    if (!item) return null;
    items.push(item);
    end = ITEM.lastIndex;
  }
  return items.length > 0 && body.slice(end) === "\n" ? items : null;
}

function parseItem(
  tag: string,
  attrs: Map<string, string>,
  body: string | undefined,
): ChatContextItem | null {
  if (tag === "quoted_text") {
    const text = body == null ? null : unquoteLines(body.split("\n"));
    return text ? { kind: "quote", text } : null;
  }

  const path = attrs.get("path");
  if (!path) return null;

  if (tag === "code_selection") {
    if (body != null) return null;
    const range = /^(\d+)(?:-(\d+))?$/.exec(attrs.get("lines") ?? "");
    if (!range) return null;
    const startLine = Number(range[1]);
    const endLine = Number(range[2] ?? range[1]);
    if (startLine < 1 || endLine < startLine) return null;
    return { kind: "code", path, startLine, endLine };
  }

  const change = attrs.get("change") as DiffLineChange | undefined;
  if (!change || !CHANGES.includes(change) || body == null) return null;
  const lineAttr = attrs.get("line");
  const line = lineAttr == null ? undefined : Number(lineAttr);
  if (line != null && !(Number.isSafeInteger(line) && line > 0)) return null;

  const lines = body.split("\n");
  const gap = lines.indexOf("");
  if (gap < 1) return null;
  const code = unquoteLines(lines.slice(0, gap));
  const comment = unescapeBody(lines.slice(gap + 1).join("\n"));
  if (code == null || !comment) return null;
  return {
    kind: "comment",
    path,
    ...(line != null ? { line } : {}),
    change,
    code,
    comment,
  };
}

function attributes(source: string): Map<string, string> {
  const attrs = new Map<string, string>();
  for (const [, name, value] of source.matchAll(ATTRIBUTE)) {
    attrs.set(name!, unescapeAttribute(value!));
  }
  return attrs;
}

function quoteLines(text: string): string {
  return escapeBody(text)
    .split("\n")
    .map((line) => (line ? `> ${line}` : ">"))
    .join("\n");
}

function unquoteLines(lines: string[]): string | null {
  const out: string[] = [];
  for (const line of lines) {
    if (line === ">") out.push("");
    else if (line.startsWith("> ")) out.push(line.slice(2));
    else return null;
  }
  return unescapeBody(out.join("\n"));
}

function escapeBody(text: string): string {
  return text.replace(RESERVED_TAG, "<\\$1$2$3");
}

function unescapeBody(text: string): string {
  return text.replace(ESCAPED_TAG, "<$1$2$3");
}

function escapeAttribute(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/\n/g, "&#10;");
}

function unescapeAttribute(value: string): string {
  return value.replace(
    /&(amp|quot|lt|gt|#10);/g,
    (_, name: string) => ENTITIES[name]!,
  );
}

function normalize(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

/** The first non-empty line, with runs of whitespace collapsed. */
export function contextExcerpt(text: string): string {
  const line = text
    .split("\n")
    .map((part) => part.trim())
    .find(Boolean);
  return (line ?? "").replace(/\s+/g, " ");
}
