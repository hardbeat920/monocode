import { fromMarkdown } from "mdast-util-from-markdown";
import { gfmFromMarkdown } from "mdast-util-gfm";
import { gfm } from "micromark-extension-gfm";

type MarkdownNode = {
  type: string;
  value?: string;
  lang?: string | null;
  position?: { start: { offset?: number }; end: { offset?: number } };
  data?: unknown;
  children?: MarkdownNode[];
};

// KaTeX layout time grows roughly quadratically with input length, and it runs
// on the UI thread, so anything longer is shown as its LaTeX source instead.
const MAX_MATH_CHARS = 5000;

function isDollarAmount(node: MarkdownNode, source: string): boolean {
  const value = node.value ?? "";
  if (/^\s|\s$/.test(value)) return true;
  const after = source.charAt(node.position?.end.offset ?? -1);
  return /\d/.test(after);
}

function rawSource(node: MarkdownNode, source: string): string {
  const start = node.position?.start.offset;
  const end = node.position?.end.offset;
  return start !== undefined && end !== undefined
    ? source.slice(start, end)
    : `$${node.value ?? ""}$`;
}

// The math tokenizer swallowed this span before inline markdown ran, so parse
// it again without math to bring back its code spans, emphasis and links.
function reparseInline(raw: string): MarkdownNode[] {
  const tree = fromMarkdown(raw, {
    extensions: [gfm()],
    mdastExtensions: [gfmFromMarkdown()],
  }) as MarkdownNode;
  const [first] = tree.children ?? [];
  if (tree.children?.length === 1 && first.type === "paragraph") {
    return first.children ?? [];
  }
  return [{ type: "text", value: raw }];
}

/**
 * Undo `$...$` parses that are not math, following Pandoc's rule: no space just
 * inside either dollar, and no digit right after the closing one. That keeps
 * "$5 and $10" as prose. Also drops oversized math back to its source.
 */
export function remarkMathGuard() {
  return (tree: MarkdownNode, file: { value?: unknown }) => {
    const source = typeof file.value === "string" ? file.value : "";
    function visit(node: MarkdownNode) {
      const children = node.children;
      if (!children) return;
      node.children = children.flatMap((child) => {
        const value = child.value ?? "";
        if (child.type === "inlineMath") {
          if (value.length > MAX_MATH_CHARS) {
            return [{ type: "text", value: rawSource(child, source) }];
          }
          if (isDollarAmount(child, source)) {
            return reparseInline(rawSource(child, source));
          }
        } else if (child.type === "math" && value.length > MAX_MATH_CHARS) {
          return [{ type: "code", lang: "latex", value }];
        }
        visit(child);
        return [child];
      });
    }
    visit(tree);
  };
}
