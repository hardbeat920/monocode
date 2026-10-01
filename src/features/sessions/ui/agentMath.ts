import { createMathPlugin, type MathPlugin } from "@streamdown/math";
import { fromMarkdown } from "mdast-util-from-markdown";
import { gfmFromMarkdown } from "mdast-util-gfm";
import { mathFromMarkdown } from "mdast-util-math";
import { gfm } from "micromark-extension-gfm";
import { math } from "micromark-extension-math";
import { parseMarkdownIntoBlocks } from "streamdown";

// KaTeX layout time grows roughly quadratically with input length, and it runs
// on the UI thread, so anything longer is shown as its LaTeX source instead.
export const MAX_MATH_CHARS = 5000;
// Per rendered block, so many small expressions cannot add up to the same stall.
const MAX_BLOCK_MATH_CHARS = 20000;
const MAX_BLOCK_MATH_EXPRESSIONS = 500;

// rehype-katex typesets any element carrying one of these classes.
const MATH_CLASSES = ["language-math", "math-inline", "math-display"];

type HastNode = {
  type: string;
  tagName?: string;
  value?: string;
  properties?: { className?: unknown };
  children?: HastNode[];
};

function textLength(node: HastNode): number {
  if (node.type === "text") return node.value?.length ?? 0;
  return (node.children ?? []).reduce(
    (sum, child) => sum + textLength(child),
    0,
  );
}

function isMathElement(node: HastNode): boolean {
  const className = node.properties?.className;
  return (
    Array.isArray(className) &&
    className.some((name) => MATH_CLASSES.includes(name))
  );
}

function showAsSource(node: HastNode) {
  const className = node.properties?.className as unknown[];
  const kept = className.filter((name) => !MATH_CLASSES.includes(String(name)));
  node.properties = {
    ...node.properties,
    className: [...kept, "language-latex"],
  };
}

function rehypeMathSizeLimit() {
  return (tree: HastNode) => {
    let chars = 0;
    let expressions = 0;
    function visit(node: HastNode, parent?: HastNode) {
      if (isMathElement(node)) {
        // rehype-katex typesets the whole `<pre>` around a math `<code>`.
        const scope =
          node.tagName === "code" && parent?.tagName === "pre" ? parent : node;
        const length = textLength(scope);
        chars += length;
        expressions += 1;
        if (
          length > MAX_MATH_CHARS ||
          chars > MAX_BLOCK_MATH_CHARS ||
          expressions > MAX_BLOCK_MATH_EXPRESSIONS
        ) {
          showAsSource(node);
        }
        return;
      }
      for (const child of node.children ?? []) visit(child, node);
    }
    visit(tree);
  };
}

const katexMath = createMathPlugin({ singleDollarTextMath: true });

/**
 * KaTeX math for agent markdown. The size limit runs immediately before KaTeX,
 * so it covers `$...$`, `$$...$$`, fenced `math` blocks and raw HTML alike.
 */
export const agentMathPlugin: MathPlugin = {
  ...katexMath,
  rehypePlugin: { plugins: [rehypeMathSizeLimit, katexMath.rehypePlugin] },
};

type MarkdownNode = {
  type: string;
  position?: { start: { offset?: number }; end: { offset?: number } };
  children?: MarkdownNode[];
};

// Inline math cannot cross these, so dollars pair up within each one.
const PHRASING_BLOCKS = new Set(["paragraph", "heading", "tableCell"]);

function isEscaped(source: string, index: number): boolean {
  let slashes = 0;
  while (source[index - 1 - slashes] === "\\") slashes++;
  return slashes % 2 === 1;
}

function isLoneDollar(source: string, index: number): boolean {
  return (
    source[index - 1] !== "$" &&
    source[index + 1] !== "$" &&
    !isEscaped(source, index)
  );
}

// Collects single `$` offsets from prose only, so code, raw HTML and `$$` math
// keep their dollars untouched.
function collectDollars(
  node: MarkdownNode,
  source: string,
  groups: number[][],
  group?: number[],
) {
  let current = group;
  if (PHRASING_BLOCKS.has(node.type)) {
    current = [];
    groups.push(current);
  }
  const start = node.position?.start.offset;
  const end = node.position?.end.offset;
  // Bare and `<...>` autolinks: a backslash there would land in the URL.
  if (node.type === "link" && start !== undefined && source[start] !== "[") {
    return;
  }
  if (
    node.type === "text" &&
    current &&
    start !== undefined &&
    end !== undefined
  ) {
    for (
      let i = source.indexOf("$", start);
      i !== -1 && i < end;
      i = source.indexOf("$", i + 1)
    ) {
      if (isLoneDollar(source, i)) current.push(i);
    }
  }
  for (const child of node.children ?? [])
    collectDollars(child, source, groups, current);
}

const isSpace = (char: string) => char === "" || /\s/u.test(char);
const isPunctuation = (char: string) => /[\p{P}\p{S}]/u.test(char);

// CommonMark's flanking rules, as for `*`: a `$` opens when text follows it,
// and closes when text precedes it, so "*$x$*" pairs inside the emphasis.
function canOpen(source: string, i: number): boolean {
  const before = source.charAt(i - 1);
  const after = source.charAt(i + 1);
  return (
    !isSpace(after) &&
    (!isPunctuation(after) || isSpace(before) || isPunctuation(before))
  );
}

function canClose(source: string, i: number): boolean {
  const before = source.charAt(i - 1);
  const after = source.charAt(i + 1);
  return (
    !isSpace(before) &&
    (!isPunctuation(before) || isSpace(after) || isPunctuation(after)) &&
    !/\d/.test(after)
  );
}

// Like Pandoc, a closing `$` may not be followed by a digit. A `$` that cannot
// close ends the pending span, so "$5. Use $x^2$" keeps `$x^2$` as math.
function literalDollars(positions: number[], source: string): number[] {
  const literal: number[] = [];
  let opener: number | undefined;
  for (const i of positions) {
    if (opener !== undefined && canClose(source, i)) {
      opener = undefined;
      continue;
    }
    if (opener !== undefined) literal.push(opener);
    opener = canOpen(source, i) ? i : undefined;
    if (opener === undefined) literal.push(i);
  }
  if (opener !== undefined) literal.push(opener);
  return literal;
}

/**
 * Escapes every single `$` that would not delimit math, before the markdown
 * parser pairs it up. Prices like "$5 and $10" stay prose with their
 * surrounding emphasis and links, and later equations still typeset.
 */
export function escapeNonMathDollars(markdown: string): string {
  if (!markdown.includes("$")) return markdown;
  const tree = fromMarkdown(markdown, {
    extensions: [gfm(), math({ singleDollarTextMath: false })],
    mdastExtensions: [gfmFromMarkdown(), mathFromMarkdown()],
  }) as MarkdownNode;
  const groups: number[][] = [];
  collectDollars(tree, markdown, groups);
  const literal = groups.flatMap((positions) =>
    literalDollars(positions, markdown),
  );
  if (literal.length === 0) return markdown;
  literal.sort((a, b) => a - b);
  let out = "";
  let last = 0;
  for (const i of literal) {
    out += `${markdown.slice(last, i)}\\`;
    last = i;
  }
  return out + markdown.slice(last);
}

// Larger blocks skip escaping rather than pay for a second parse of them.
const MAX_ESCAPE_BLOCK_CHARS = 50000;
const ESCAPE_CACHE_SIZE = 200;
const escapedBlocks = new Map<string, string>();

function escapeBlock(block: string): string {
  if (!block.includes("$") || block.length > MAX_ESCAPE_BLOCK_CHARS) {
    return block;
  }
  const cached = escapedBlocks.get(block);
  if (cached !== undefined) return cached;
  const escaped = escapeNonMathDollars(block);
  escapedBlocks.set(block, escaped);
  if (escapedBlocks.size > ESCAPE_CACHE_SIZE) {
    const oldest = escapedBlocks.keys().next().value;
    if (oldest !== undefined) escapedBlocks.delete(oldest);
  }
  return escaped;
}

/**
 * Streamdown's block splitter with dollar escaping per block. While a reply
 * streams only its last block changes, so finished blocks come from the cache.
 */
export function parseAgentMarkdownBlocks(markdown: string): string[] {
  return parseMarkdownIntoBlocks(markdown).map(escapeBlock);
}
