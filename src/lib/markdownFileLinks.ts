import { resolveWorkspaceFileReference } from "./paths";

type MarkdownNode = {
  type: string;
  url?: string;
  children?: MarkdownNode[];
};

/** Normalize local file links before URL sanitizing; keep all other URLs intact. */
export function remarkWorkspaceFileLinks({ cwd }: { cwd?: string }) {
  function visit(node: MarkdownNode) {
    if ((node.type === "link" || node.type === "definition") && node.url) {
      const file = resolveWorkspaceFileReference(node.url, cwd);
      if (file) {
        const path = file.path.startsWith("/") ? file.path : `/${file.path}`;
        let href = path.split("/").map(encodeURIComponent).join("/");
        // Preserve a UNC path as a path, not a protocol-relative web origin.
        if (href.startsWith("//")) href = `/%2F${href.slice(2)}`;
        const target = file.navigation;
        node.url =
          href +
          (target
            ? `:${target.line}${target.column ? `:${target.column}` : ""}`
            : "");
      }
    }
    for (const child of node.children ?? []) visit(child);
  }
  return visit;
}
