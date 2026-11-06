// A small Markdown reader for agent replies: the subset agents actually write.
// The desktop renders the same text with remark; this keeps the phone free of
// a parser dependency while reading headings, lists, code and emphasis alike.

/** A picture a provider's text shows, with the size its `<img>` asked for. */
export type MarkdownImage = {
  type: "image";
  src: string;
  alt: string;
  width?: number;
  height?: number;
};

export type Inline =
  /** `word`: the place of its first word in the reply, set by numberWords. */
  | { type: "text"; text: string; word?: number }
  | { type: "code"; text: string }
  | { type: "strong"; children: Inline[] }
  | { type: "em"; children: Inline[] }
  | { type: "strike"; children: Inline[] }
  | { type: "link"; href: string; children: Inline[] }
  | MarkdownImage;

export type MarkdownBlock =
  | { type: "paragraph"; inline: Inline[] }
  | { type: "heading"; level: number; inline: Inline[] }
  | { type: "code"; lang: string; text: string }
  | {
      type: "list";
      ordered: boolean;
      start: number;
      items: MarkdownBlock[][];
      /** Each item's task box, in a list that has any: ticked or not. */
      tasks?: (boolean | undefined)[];
    }
  | { type: "quote"; children: MarkdownBlock[] }
  | { type: "table"; rows: Inline[][][] }
  | { type: "rule" }
  | { type: "image"; image: MarkdownImage; href?: string }
  | { type: "details"; open: boolean; summary: Inline[]; children: MarkdownBlock[] };

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([\w+-]*)/;
const HEADING = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^ {0,3}([-*_])(\s*\1){2,}\s*$/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const QUOTE = /^ {0,3}>\s?(.*)$/;
const TABLE_DIVIDER = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)+\|?\s*$/;
const TASK = /^\[([ xX])\]\s+(.*)$/;

// The HTML people write in pull requests and issues. Agent replies never read
// it: there, a tag someone is explaining stays on the page.
const CODE_OR_COMMENT = /(`+)[^`]*?\1|<!--.*?(?:-->|$)/g;
const DETAILS = /^\s*<details\b([^>]*)>/i;
const SUMMARY = /^\s*<summary\b[^>]*>([\s\S]*?)<\/summary\s*>/i;
const TAG = /^<(\/?)([a-z][a-z0-9]*)\b([^<>]*)>/i;
const IMAGE = /^!\[([^\]]*)\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/;
const LINKED_IMAGE = /^\[\s*(!\[[^\]]*\]\([^)]*\))\s*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/;
const EMPHASIS_TAGS: Record<string, "strong" | "em" | "strike"> = {
  b: "strong",
  strong: "strong",
  i: "em",
  em: "em",
  s: "strike",
  strike: "strike",
  del: "strike",
};
/** Tags dropped for their contents. Only real ones, so `Vec<T>` in prose stays. */
const LAYOUT_TAGS = new Set(
  (
    "abbr blockquote center details div figcaption figure h1 h2 h3 h4 h5 h6 hr ins kbd li " +
    "mark ol p picture pre samp small source span sub summary sup table tbody td tfoot th " +
    "thead tr u ul var video"
  ).split(" "),
);

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
      case "details":
        inline(node.summary);
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

/** `html` reads a provider's text, which mixes HTML in, rather than an agent's. */
export function parseMarkdown(
  source: string,
  { html = false }: { html?: boolean } = {},
): MarkdownBlock[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  return parseBlocks(html ? withoutComments(lines) : lines, html);
}

/** The lines without HTML comments, which may span lines. Code keeps its own. */
function withoutComments(lines: string[]): string[] {
  const kept: string[] = [];
  let fence = "";
  let inComment = false;
  for (const raw of lines) {
    let line = raw;
    if (inComment) {
      const end = line.indexOf("-->");
      if (end < 0) continue;
      inComment = false;
      line = line.slice(end + 3);
    }
    if (fence) {
      if (line.trimStart().startsWith(fence)) fence = "";
      kept.push(line);
      continue;
    }
    const opened = FENCE.exec(line);
    if (opened) {
      fence = opened[1];
      kept.push(line);
      continue;
    }
    kept.push(
      line.replace(CODE_OR_COMMENT, (match) => {
        if (!match.startsWith("<!--")) return match;
        inComment = !match.endsWith("-->");
        return "";
      }),
    );
  }
  return kept;
}

function parseBlocks(lines: string[], html: boolean): MarkdownBlock[] {
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
        inline: parseInline(heading[2], html),
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
      blocks.push({ type: "quote", children: parseBlocks(body, html) });
      continue;
    }

    const details = html ? DETAILS.exec(line) : null;
    if (details) {
      const [block, next] = parseDetails(lines, index, details);
      blocks.push(block);
      index = next;
      continue;
    }

    if (line.includes("|") && TABLE_DIVIDER.test(lines[index + 1] ?? "")) {
      const rows = [cells(line, html)];
      index += 2;
      while (index < lines.length && lines[index].includes("|") && lines[index].trim()) {
        rows.push(cells(lines[index], html));
        index += 1;
      }
      blocks.push({ type: "table", rows });
      continue;
    }

    const item = ITEM.exec(line);
    if (item) {
      const [list, next] = parseList(lines, index, html);
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
      !(paragraph.length > 0 && ITEM.test(lines[index])) &&
      !(paragraph.length > 0 && html && DETAILS.test(lines[index]))
    ) {
      paragraph.push(lines[index].trim());
      index += 1;
    }
    const inline = parseInline(paragraph.join("\n"), html);
    if (!html) {
      blocks.push({ type: "paragraph", inline });
      continue;
    }
    // An image stands on its own, between what is written around it.
    for (const piece of splitImages(inline)) {
      blocks.push(
        piece.kind === "image"
          ? { type: "image", image: piece.image, href: piece.href }
          : { type: "paragraph", inline: piece.inline },
      );
    }
  }
  return blocks;
}

/** A `<details>` from its opening line to the `</details>` that matches it. */
function parseDetails(
  lines: string[],
  from: number,
  opening: RegExpExecArray,
): [MarkdownBlock, number] {
  let depth = 0;
  let index = from;
  while (index < lines.length) {
    depth += (lines[index].match(/<details\b/gi) ?? []).length;
    depth -= (lines[index].match(/<\/details\s*>/gi) ?? []).length;
    index += 1;
    if (depth <= 0) break;
  }
  const text = lines.slice(from, index).join("\n");
  const close = text.toLowerCase().lastIndexOf("</details");
  const inner = text.slice(opening[0].length, close < 0 ? undefined : close);
  const summary = SUMMARY.exec(inner);
  const body = summary ? inner.slice(summary[0].length) : inner;
  return [
    {
      type: "details",
      open: /\bopen\b/i.test(opening[1]),
      summary: parseInline(summary?.[1].trim() || "Details", true),
      children: parseBlocks(body.split("\n"), true),
    },
    index,
  ];
}

function parseList(lines: string[], from: number, html: boolean): [MarkdownBlock, number] {
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
  const tasks = items.map((body) => {
    const task = TASK.exec(body[0]);
    if (!task) return undefined;
    body[0] = task[2];
    return task[1] !== " ";
  });
  return [
    {
      type: "list",
      ordered,
      start: ordered ? parseInt(first[2], 10) : 1,
      items: items.map((body) => parseBlocks(body, html)),
      ...(tasks.some((task) => task !== undefined) ? { tasks } : {}),
    },
    index,
  ];
}

function cells(row: string, html: boolean): Inline[][] {
  return row
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => parseInline(cell.trim(), html));
}

const DELIMITERS: [string, "strong" | "em" | "strike"][] = [
  ["**", "strong"],
  ["__", "strong"],
  ["~~", "strike"],
  ["*", "em"],
  ["_", "em"],
];

/** The named attribute of a tag's attributes, quoted or not. */
function attribute(attributes: string, name: string): string | undefined {
  const found = new RegExp(
    `(?:^|\\s)${name}\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s"'>]+))`,
    "i",
  ).exec(attributes);
  return found?.[1] ?? found?.[2] ?? found?.[3];
}

/** A size in pixels. A percentage says nothing about the picture's shape. */
function pixels(value: string | undefined): number | undefined {
  const found = /^\s*(\d+)(?:px)?\s*$/.exec(value ?? "");
  return found ? parseInt(found[1], 10) : undefined;
}

/** What the HTML tag at `at` reads as, or null when it is not one, as `Vec<T>`. */
function parseTag(text: string, at: number): { nodes: Inline[]; end: number } | null {
  const tag = TAG.exec(text.slice(at));
  if (!tag) return null;
  const [whole, closing, rawName, attributes] = tag;
  const name = rawName.toLowerCase();
  const end = at + whole.length;
  if (name === "img") {
    const src = attribute(attributes, "src");
    if (!src) return { nodes: [], end };
    const image: MarkdownImage = { type: "image", src, alt: attribute(attributes, "alt") ?? "" };
    const width = pixels(attribute(attributes, "width"));
    const height = pixels(attribute(attributes, "height"));
    return { nodes: [{ ...image, ...(width ? { width } : {}), ...(height ? { height } : {}) }], end };
  }
  if (name === "br") return { nodes: [{ type: "text", text: "\n" }], end };
  const emphasis = EMPHASIS_TAGS[name];
  if (!closing && (emphasis || name === "a" || name === "code")) {
    const close = text.toLowerCase().indexOf(`</${name}`, end);
    if (close >= 0) {
      const inner = text.slice(end, close);
      const after = text.indexOf(">", close);
      const next = after < 0 ? text.length : after + 1;
      if (name === "code") return { nodes: [{ type: "code", text: inner.trim() }], end: next };
      const children = parseInline(inner, true);
      if (emphasis) return { nodes: [{ type: emphasis, children }], end: next };
      const href = attribute(attributes, "href");
      return { nodes: href ? [{ type: "link", href, children }] : children, end: next };
    }
  }
  return emphasis || name === "a" || name === "code" || LAYOUT_TAGS.has(name)
    ? { nodes: [], end }
    : null;
}

export function parseInline(text: string, html = false): Inline[] {
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
    if (html && char === "<") {
      const tag = parseTag(text, index);
      if (tag) {
        flush();
        out.push(...tag.nodes);
        index = tag.end;
        continue;
      }
    }
    if (html && char === "!") {
      const image = IMAGE.exec(text.slice(index));
      if (image) {
        flush();
        out.push({ type: "image", src: image[2], alt: image[1] });
        index += image[0].length;
        continue;
      }
    }
    if (char === "[") {
      const rest = text.slice(index);
      // `[^\]]` stops at the image's own bracket, so a linked one is read first.
      const link = (html ? LINKED_IMAGE.exec(rest) : null) ?? /^\[([^\]]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/.exec(rest);
      if (link) {
        flush();
        out.push({ type: "link", href: link[2], children: parseInline(link[1], html) });
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
        out.push({ type, children: parseInline(text.slice(index + mark.length, close), html) });
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

export function holdsImage(node: Inline): boolean {
  return node.type === "image" || ("children" in node && node.children.some(holdsImage));
}

export type InlinePiece =
  | { kind: "text"; inline: Inline[] }
  | { kind: "image"; image: MarkdownImage; href?: string };

/** The space an image left at the edge of the text beside it goes with it. */
function trimmedPiece(inline: Inline[]): Inline[] {
  const blank = (node: Inline | undefined) => node?.type === "text" && !node.text.trim();
  let start = 0;
  let end = inline.length;
  while (start < end && blank(inline[start])) start += 1;
  while (end > start && blank(inline[end - 1])) end -= 1;
  const nodes = inline.slice(start, end);
  const first = nodes[0];
  if (first?.type === "text") nodes[0] = { ...first, text: first.text.trimStart() };
  const last = nodes.at(-1);
  if (last?.type === "text") nodes[nodes.length - 1] = { ...last, text: last.text.trimEnd() };
  return nodes;
}

/**
 * Text cut apart at its images, in order. An image inside a link or emphasis
 * comes out on its own, keeping the link; what remains of the text keeps its
 * marks. Text that was only tags and space is gone.
 */
export function splitImages(nodes: Inline[], href?: string): InlinePiece[] {
  const pieces: InlinePiece[] = [];
  const addText = (node: Inline) => {
    const last = pieces.at(-1);
    if (last?.kind === "text") last.inline.push(node);
    else pieces.push({ kind: "text", inline: [node] });
  };
  for (const node of nodes) {
    if (node.type === "image") {
      pieces.push({ kind: "image", image: node, href });
    } else if ("children" in node && holdsImage(node)) {
      const link = node.type === "link" ? node.href : href;
      for (const piece of splitImages(node.children, link)) {
        if (piece.kind === "image") pieces.push(piece);
        else addText({ ...node, children: piece.inline });
      }
    } else {
      addText(node);
    }
  }
  return pieces.flatMap((piece): InlinePiece[] => {
    if (piece.kind === "image") return [piece];
    const inline = trimmedPiece(piece.inline);
    return inline.length > 0 ? [{ kind: "text", inline }] : [];
  });
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

/** A message's words without its markdown, for a one-line preview. */
export function plainText(source: string): string {
  const inline = (parts: Inline[]): string =>
    parts
      .map((part) =>
        part.type === "image" ? part.alt : "children" in part ? inline(part.children) : part.text,
      )
      .join("");
  const block = (item: MarkdownBlock): string => {
    switch (item.type) {
      case "paragraph":
      case "heading":
        return inline(item.inline);
      case "code":
        return item.text;
      case "list":
        return item.items.map((entry) => entry.map(block).join(" ")).join(" ");
      case "quote":
        return item.children.map(block).join(" ");
      case "details":
        return [inline(item.summary), ...item.children.map(block)].join(" ");
      case "table":
        return item.rows.map((row) => row.map(inline).join(" ")).join(" ");
      case "image":
        return item.image.alt;
      case "rule":
        return "";
    }
  };
  return parseMarkdown(source).map(block).join(" ").replace(/\s+/g, " ").trim();
}
