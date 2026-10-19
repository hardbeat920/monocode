// A small Markdown reader for agent replies: the subset agents actually write.
// The desktop renders the same text with remark; this keeps the phone free of
// a parser dependency while reading headings, lists, code and emphasis alike.

export type Inline =
  /** `word`: the place of its first word in the reply, set by numberWords. */
  | { type: "text"; text: string; word?: number }
  | { type: "code"; text: string }
  | { type: "strong"; children: Inline[] }
  | { type: "em"; children: Inline[] }
  | { type: "strike"; children: Inline[] }
  | { type: "link"; href: string; children: Inline[] };

export type MarkdownBlock =
  | { type: "paragraph"; inline: Inline[] }
  | { type: "heading"; level: number; inline: Inline[] }
  | { type: "code"; lang: string; text: string }
  | { type: "list"; ordered: boolean; start: number; items: MarkdownBlock[][] }
  | { type: "quote"; children: MarkdownBlock[] }
  | { type: "table"; rows: Inline[][][] }
  | { type: "rule" };

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([\w+-]*)/;
const HEADING = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^ {0,3}([-*_])(\s*\1){2,}\s*$/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const QUOTE = /^ {0,3}>\s?(.*)$/;
const TABLE_DIVIDER = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)+\|?\s*$/;

/** A run of text split into words and the space between them. */
export function splitWords(text: string): string[] {
  return text.split(/(\s+)/).filter(Boolean);
}

/**
 * Numbers the words a reply shows, in reading order, and returns how many
 * there are. Text only grows as it streams, so a word keeps its number from
 * one render to the next and only new words fade. Links and code are read
 * whole and appear as they are, as on desktop.
 */
export function numberWords(blocks: MarkdownBlock[]): {
  count: number;
  ranges: [number, number][];
} {
  let count = 0;
  const inline = (nodes: Inline[]) => {
    for (const node of nodes) {
      if (node.type === "text") {
        node.word = count;
        count += splitWords(node.text).filter((part) => !/^\s/.test(part)).length;
      } else if (node.type === "strong" || node.type === "em" || node.type === "strike")
        inline(node.children);
    }
  };
  const block = (node: MarkdownBlock) => {
    switch (node.type) {
      case "paragraph":
      case "heading":
        inline(node.inline);
        break;
      case "list":
        node.items.forEach((item) => item.forEach(block));
        break;
      case "quote":
        node.children.forEach(block);
        break;
      case "table":
        node.rows.forEach((row) => row.forEach(inline));
        break;
    }
  };
  const ranges = blocks.map((node): [number, number] => {
    const start = count;
    block(node);
    return [start, count];
  });
  return { count, ranges };
}

export function parseMarkdown(source: string): MarkdownBlock[] {
  return parseBlocks(source.replace(/\r\n?/g, "\n").split("\n"));
}

function parseBlocks(lines: string[]): MarkdownBlock[] {
  const blocks: MarkdownBlock[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    if (!line.trim()) {
      index += 1;
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      const close = fence[1];
      const body: string[] = [];
      index += 1;
      // An unclosed fence, as while the reply streams, runs to the end.
      while (index < lines.length && !lines[index].trimStart().startsWith(close)) {
        body.push(lines[index]);
        index += 1;
      }
      index += 1;
      blocks.push({ type: "code", lang: fence[2], text: body.join("\n") });
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      blocks.push({
        type: "heading",
        level: heading[1].length,
        inline: parseInline(heading[2]),
      });
      index += 1;
      continue;
    }

    if (RULE.test(line)) {
      blocks.push({ type: "rule" });
      index += 1;
      continue;
    }

    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (index < lines.length && lines[index].trim()) {
        const quoted = QUOTE.exec(lines[index]);
        body.push(quoted ? quoted[1] : lines[index]);
        index += 1;
      }
      blocks.push({ type: "quote", children: parseBlocks(body) });
      continue;
    }

    if (line.includes("|") && TABLE_DIVIDER.test(lines[index + 1] ?? "")) {
      const rows = [cells(line)];
      index += 2;
      while (index < lines.length && lines[index].includes("|") && lines[index].trim()) {
        rows.push(cells(lines[index]));
        index += 1;
      }
      blocks.push({ type: "table", rows });
      continue;
    }

    const item = ITEM.exec(line);
    if (item) {
      const [list, next] = parseList(lines, index);
      blocks.push(list);
      index = next;
      continue;
    }

    const paragraph: string[] = [];
    while (
      index < lines.length &&
      lines[index].trim() &&
      !FENCE.test(lines[index]) &&
      !HEADING.test(lines[index]) &&
      !QUOTE.test(lines[index]) &&
      !(paragraph.length > 0 && ITEM.test(lines[index]))
    ) {
      paragraph.push(lines[index].trim());
      index += 1;
    }
    blocks.push({ type: "paragraph", inline: parseInline(paragraph.join("\n")) });
  }
  return blocks;
}

function parseList(lines: string[], from: number): [MarkdownBlock, number] {
  const first = ITEM.exec(lines[from])!;
  const indent = first[1].length;
  const ordered = /\d/.test(first[2]);
  const items: string[][] = [];
  let index = from;
  while (index < lines.length) {
    const line = lines[index];
    const item = ITEM.exec(line);
    if (item && item[1].length === indent && /\d/.test(item[2]) === ordered) {
      items.push([item[3]]);
      index += 1;
      continue;
    }
    const blank = !line.trim();
    const nested = line.length - line.trimStart().length > indent;
    // A blank line ends the list unless the next line carries on inside it.
    if (blank) {
      const next = lines[index + 1] ?? "";
      const nextItem = ITEM.exec(next);
      if (
        (nextItem && nextItem[1].length === indent) ||
        (next.trim() && next.length - next.trimStart().length > indent)
      ) {
        items[items.length - 1].push("");
        index += 1;
        continue;
      }
      break;
    }
    if (nested) {
      items[items.length - 1].push(line.slice(Math.min(indent + 2, line.length - line.trimStart().length)));
      index += 1;
      continue;
    }
    // A lazy continuation of the item's paragraph.
    if (!item && !FENCE.test(line) && !HEADING.test(line)) {
      items[items.length - 1].push(line.trim());
      index += 1;
      continue;
    }
    break;
  }
  return [
    {
      type: "list",
      ordered,
      start: ordered ? parseInt(first[2], 10) : 1,
      items: items.map((body) => parseBlocks(body)),
    },
    index,
  ];
}

function cells(row: string): Inline[][] {
  return row
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => parseInline(cell.trim()));
}

const DELIMITERS: [string, "strong" | "em" | "strike"][] = [
  ["**", "strong"],
  ["__", "strong"],
  ["~~", "strike"],
  ["*", "em"],
  ["_", "em"],
];

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let plain = "";
  const flush = () => {
    if (plain) out.push({ type: "text", text: plain });
    plain = "";
  };
  let index = 0;
  while (index < text.length) {
    const char = text[index];
    if (char === "\\" && index + 1 < text.length && /[\\`*_~[\]()#>+-.!|]/.test(text[index + 1])) {
      plain += text[index + 1];
      index += 2;
      continue;
    }
    if (char === "`") {
      const ticks = /^`+/.exec(text.slice(index))![0];
      const close = text.indexOf(ticks, index + ticks.length);
      if (close > 0) {
        flush();
        out.push({ type: "code", text: text.slice(index + ticks.length, close).trim() });
        index = close + ticks.length;
        continue;
      }
    }
    if (char === "[") {
      const link = /^\[([^\]]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/.exec(text.slice(index));
      if (link) {
        flush();
        out.push({ type: "link", href: link[2], children: parseInline(link[1]) });
        index += link[0].length;
        continue;
      }
    }
    if (char === "h" && /^https?:\/\//.test(text.slice(index, index + 8)) && !/\w/.test(text[index - 1] ?? "")) {
      const url = /^https?:\/\/[^\s<>()]+[^\s<>().,;:!?'"]/.exec(text.slice(index));
      if (url) {
        flush();
        out.push({ type: "link", href: url[0], children: [{ type: "text", text: url[0] }] });
        index += url[0].length;
        continue;
      }
    }
    const delimiter = DELIMITERS.find(([mark]) => text.startsWith(mark, index));
    if (delimiter) {
      const [mark, type] = delimiter;
      const after = text[index + mark.length];
      // Emphasis opens against a word, and `_` never inside one (snake_case).
      const opens =
        after !== undefined &&
        !/\s/.test(after) &&
        !(mark[0] === "_" && /\w/.test(text[index - 1] ?? ""));
      const close = opens ? findClose(text, mark, index + mark.length) : -1;
      if (close > 0) {
        flush();
        out.push({ type, children: parseInline(text.slice(index + mark.length, close)) });
        index = close + mark.length;
        continue;
      }
    }
    plain += char;
    index += 1;
  }
  flush();
  return out;
}

function findClose(text: string, mark: string, from: number): number {
  let index = text.indexOf(mark, from);
  while (index > 0) {
    const before = text[index - 1];
    const after = text[index + mark.length];
    const wordAfter = mark[0] === "_" && after !== undefined && /\w/.test(after);
    // `**` is not a closing `*`.
    const doubled = mark.length === 1 && (after === mark || text[index - 1] === mark);
    if (!/\s/.test(before) && !wordAfter && !doubled) return index;
    index = text.indexOf(mark, index + mark.length);
  }
  return -1;
}
