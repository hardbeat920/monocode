import type { Plugin, ViteDevServer } from "vite";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
declare const process: any;

// @ts-expect-error child_process is a nodejs built-in
import { exec } from "child_process";

export interface ComponentInspectorOptions {
  enabled?: boolean;
  [key: string]: unknown;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function cursorInspectorBabelPlugin({ types: t }: any) {
  return {
    visitor: {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      JSXOpeningElement(path: any, state: any) {
        const filename = state.file?.opts?.filename;
        if (!filename || !filename.includes("/src/")) return;
        const loc = path.node.loc;
        if (!loc) return;

        let comp = "";
        let curr = path;
        while (curr) {
          if (
            (curr.isFunctionDeclaration() || curr.isFunctionExpression()) &&
            curr.node.id
          ) {
            comp = curr.node.id.name;
            break;
          }
          if (curr.isVariableDeclarator() && t.isIdentifier(curr.node.id)) {
            comp = curr.node.id.name;
            break;
          }
          curr = curr.parentPath;
        }

        if (!comp && filename) {
          const base = filename.split("/").pop()?.replace(/\.[^.]+$/, "") || "";
          if (/^[A-Z]/.test(base)) {
            comp = base;
          }
        }

        const nameNode = path.node.name;
        const tagName = t.isJSXIdentifier(nameNode) ? nameNode.name : "";

        path.node.attributes.push(
          t.jsxAttribute(t.jsxIdentifier("data-insp-file"), t.stringLiteral(filename)),
          t.jsxAttribute(t.jsxIdentifier("data-insp-line"), t.stringLiteral(String(loc.start.line))),
          t.jsxAttribute(t.jsxIdentifier("data-insp-col"), t.stringLiteral(String(loc.start.column + 1))),
          t.jsxAttribute(t.jsxIdentifier("data-insp-comp"), t.stringLiteral(comp || tagName))
        );
      },
    },
  };
}

export function getEditorCommand(
  editor: string,
  file: string,
  line: string,
  col: string
): string {
  const target = `${file}:${line}:${col}`;
  switch (editor.toLowerCase()) {
    case "code":
    case "vscode":
      return `code -g "${target}"`;
    case "code-insiders":
      return `code-insiders -g "${target}"`;
    case "windsurf":
      return `windsurf -g "${target}" || ~/.local/bin/windsurf -g "${target}" || code -g "${target}"`;
    case "zed":
      return `zed "${target}"`;
    case "webstorm":
      return `webstorm --line ${line} --column ${col} "${file}" || idea --line ${line} --column ${col} "${file}"`;
    case "subl":
    case "sublime":
      return `subl "${target}"`;
    case "cursor":
    default:
      return `cursor -g "${target}" || ~/.local/bin/cursor -g "${target}" || code -g "${target}"`;
  }
}

export function componentInspector(
  options?: ComponentInspectorOptions
): Plugin {
  const enabled = options?.enabled ?? true;

  return {
    name: "vite-plugin-component-inspector",
    apply: "serve",
    api: {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      reactBabel(babelConfig: any) {
        if (!enabled) return;
        babelConfig.plugins = babelConfig.plugins || [];
        babelConfig.plugins.push(cursorInspectorBabelPlugin);
      },
    },
    transformIndexHtml() {
      if (!enabled) return;
      return [
        {
          tag: "script",
          attrs: {
            type: "module",
            src: "/src/lib/componentInspector.ts",
          },
          injectTo: "body",
        },
      ];
    },
    configureServer(server: ViteDevServer) {
      if (!enabled) return;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const handler = (req: any, res: any) => {
        const url = new URL(req.url || "", "http://localhost");
        const file = url.searchParams.get("file");
        const line = url.searchParams.get("line") || "1";
        const col = url.searchParams.get("col") || "1";
        const editor =
          url.searchParams.get("editor") ||
          process.env.MONOCODE_INSPECTOR_EDITOR ||
          process.env.EDITOR ||
          "cursor";

        if (file) {
          const cmd = getEditorCommand(editor, file, line, col);
          exec(cmd, (err) => {
            if (err) {
              console.warn(
                `[component-inspector] Failed to open in ${editor}:`,
                err.message
              );
            }
          });
        }
        res.statusCode = 200;
        res.end("ok");
      };

      server.middlewares.use("/__open_in_editor", handler);
      server.middlewares.use("/__open_in_cursor", handler);
    },
  };
}
