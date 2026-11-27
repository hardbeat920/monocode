// Bundle the same Prettier version, parsers and defaults as shared/lib/format.ts.
import { formatWithCursor } from "prettier/standalone";
import * as estree from "prettier/plugins/estree";
import * as babel from "prettier/plugins/babel";
import * as typescript from "prettier/plugins/typescript";
import * as postcss from "prettier/plugins/postcss";
import * as html from "prettier/plugins/html";
import * as markdown from "prettier/plugins/markdown";
globalThis.monocodeFormat = async (path: string, source: string, cursorOffset: number) => {
  if (source.length > 512 * 1024) return "null";
  const ext = path.toLowerCase().split(".").pop();
  const parser = ["js", "jsx", "mjs", "cjs"].includes(ext!) ? "babel"
    : ["ts", "tsx", "mts", "cts"].includes(ext!) ? "typescript"
    : ext === "json" ? "json" : ext === "css" ? "css"
    : ext === "html" || ext === "htm" ? "html"
    : ext === "md" || ext === "markdown" ? "markdown"
    : ext === "mdx" ? "mdx" : null;
  if (!parser) return "null";
  try {
    return JSON.stringify(await formatWithCursor(source, { filepath: path, parser, cursorOffset, plugins: [estree, babel, typescript, postcss, html, markdown] }));
  } catch { return "null"; }
};
