import type { Plugin } from "prettier";
import { basename } from "../../platform/tauri/fs";

type ParserName =
  | "babel"
  | "typescript"
  | "json"
  | "css"
  | "html"
  | "markdown"
  | "mdx";

const MAX_FORMAT_CHARS = 512 * 1024;

export async function formatText(
  path: string,
  source: string,
  cursorOffset: number,
): Promise<{ formatted: string; cursorOffset: number } | null> {
  if (source.length > MAX_FORMAT_CHARS) return null;
  const parser = parserForPath(path);
  if (!parser) return null;

  try {
    const prettier = await import("prettier/standalone");
    return await prettier.formatWithCursor(source, {
      parser,
      plugins: await pluginsFor(parser),
      cursorOffset,
      filepath: path,
    });
  } catch {
    return null;
  }
}

function parserForPath(path: string): ParserName | null {
  const name = basename(path).toLowerCase();
  const extension = name.includes(".") ? name.slice(name.lastIndexOf(".")) : "";

  if ([".js", ".jsx", ".mjs", ".cjs"].includes(extension)) return "babel";
  if ([".ts", ".tsx", ".mts", ".cts"].includes(extension)) return "typescript";
  if (extension === ".json") return "json";
  if (extension === ".css") return "css";
  if ([".html", ".htm"].includes(extension)) return "html";
  if ([".md", ".markdown"].includes(extension)) return "markdown";
  if (extension === ".mdx") return "mdx";
  return null;
}

async function pluginsFor(parser: ParserName): Promise<Plugin[]> {
  switch (parser) {
    case "babel":
    case "json": {
      const [estree, babel] = await Promise.all([
        import("prettier/plugins/estree"),
        import("prettier/plugins/babel"),
      ]);
      return [estree, babel];
    }
    case "typescript": {
      const [estree, typescript] = await Promise.all([
        import("prettier/plugins/estree"),
        import("prettier/plugins/typescript"),
      ]);
      return [estree, typescript];
    }
    case "css":
      return [await import("prettier/plugins/postcss")];
    case "html":
      return [await import("prettier/plugins/html")];
    case "markdown":
    case "mdx":
      return [await import("prettier/plugins/markdown")];
  }
}
