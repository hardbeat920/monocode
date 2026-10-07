import type { Element, ElementContent, Root } from "hast";

/*
 * Markdown reads a single newline inside a block as a soft break, which HTML
 * collapses into a space, so consecutive lines run together into one line of
 * prose (#591). Notes and documents are written with hard-wrapped lines, where
 * one newline is a line break in its own right, so the preview turns each one
 * into a `<br>` the way Obsidian's "Strict line breaks" setting does.
 */

/** Blocks whose text is prose, where a newline is a line break. */
const BREAKABLE_TAGS = new Set([
  "p",
  "li",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "td",
  "th",
  "dt",
  "dd",
]);

/** Block elements: a newline next to one is layout whitespace, not a break. */
const BLOCK_TAGS = new Set([
  "p",
  "ul",
  "ol",
  "li",
  "pre",
  "blockquote",
  "table",
  "thead",
  "tbody",
  "tr",
  "td",
  "th",
  "div",
  "details",
  "summary",
  "hr",
  "dl",
  "dt",
  "dd",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "section",
  "figure",
]);

/** Text inside these is literal, so a newline there is not a line break. */
const LITERAL_TAGS = new Set([
  "pre",
  "code",
  "kbd",
  "samp",
  "textarea",
  "script",
]);

export function rehypeHardBreaks() {
  return (tree: Root) => {
    walk(tree, false, false);
  };
}

/**
 * `breakable` stays true down through inline elements (emphasis, links) so a
 * wrapped line that Markdown kept inside one of them still breaks, and
 * `afterBreak` keeps the newline mdast-to-hast writes after a `<br>` a
 * Markdown hard break already put there from adding a second one.
 */
function walk(
  parent: Root | Element,
  breakable: boolean,
  afterBreak: boolean,
): void {
  const tag = parent.type === "element" ? parent.tagName : "";
  const inside = breakable || BREAKABLE_TAGS.has(tag);
  const children: Array<Root["children"][number]> = [];
  let followsBreak = afterBreak;

  for (const [index, child] of parent.children.entries()) {
    if (
      child.type === "text" &&
      inside &&
      isWrappedLine(
        child.value,
        parent.children[index - 1],
        parent.children[index + 1],
      )
    ) {
      children.push(
        ...splitLines(
          child.value,
          followsBreak,
          isInline(parent.children[index + 1]),
        ),
      );
    } else {
      if (child.type === "element" && !LITERAL_TAGS.has(child.tagName)) {
        walk(child, inside, false);
      }
      children.push(child);
    }
    followsBreak = child.type === "element" && child.tagName === "br";
  }

  parent.children = children;
}

/**
 * A line of prose split across a newline. The whitespace mdast-to-hast writes
 * between block children is not one: it would put a break in front of every
 * nested list, loose paragraph, and code block a list item holds. The same
 * whitespace between two inline nodes (`*a*` over `*b*`) is the only newline
 * there is between those lines, so it does break.
 */
function isWrappedLine(
  value: string,
  prev: Root["children"][number] | undefined,
  next: Root["children"][number] | undefined,
): boolean {
  if (!value.includes("\n")) return false;
  if (/\S/.test(value)) return true;
  return isInline(prev) && isInline(next);
}

function isInline(node: Root["children"][number] | undefined): boolean {
  if (!node) return false;
  if (node.type === "text") return true;
  return node.type === "element" && !BLOCK_TAGS.has(node.tagName);
}

/**
 * One line's worth of text, in order: the text, then a `<br>` before every
 * line after it. Whitespace around a break is dropped so the break adds no
 * space of its own.
 *
 * A blank line at either end is kept only when it is a real break: the leading
 * one when it follows a `<br>` (which already broke there), the trailing one
 * when more prose follows it. Dropping either one unconditionally loses a line,
 * because a text node can span lines and be followed by an inline element, or
 * follow a `<br>` that came from raw HTML rather than from Markdown.
 */
function splitLines(
  value: string,
  afterBreak: boolean,
  followedByInline: boolean,
): ElementContent[] {
  if (!/\S/.test(value)) {
    // Only a newline between two inline nodes: it is the whole break.
    return afterBreak ? [] : [lineBreak()];
  }
  const lines = value.split("\n");
  if (afterBreak && lines[0].trim() === "") lines.shift();
  // A trailing newline ends the block's last line, so it separates nothing.
  while (
    !followedByInline &&
    lines.length > 0 &&
    lines[lines.length - 1].trim() === ""
  ) {
    lines.pop();
  }

  const nodes: ElementContent[] = [];
  lines.forEach((line, index) => {
    if (index > 0) {
      nodes.push(lineBreak());
    }
    const text = index === 0 ? line : line.replace(/^[ \t]+/, "");
    const trimmed =
      index === lines.length - 1 ? text : text.replace(/[ \t]+$/, "");
    if (trimmed) nodes.push({ type: "text", value: trimmed });
  });

  return nodes;
}

function lineBreak(): ElementContent {
  return { type: "element", tagName: "br", properties: {}, children: [] };
}
