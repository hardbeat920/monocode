import { invoke } from "@tauri-apps/api/core";
import type { Plugin } from "esbuild-wasm";
import wasmURL from "esbuild-wasm/esbuild.wasm?url";
import type { ModManifest } from "../api/types";
import runtimeSource from "./worker.ts?raw";

/** One discovered mod, as `mods_list` reports it. */
export type ModRecord = {
  name: string;
  dir: string;
  source: "project" | "user" | "unpacked";
  manifest: ModManifest;
  error?: string | null;
  enabled: boolean;
  manifestHash: string;
  /** Folders of same-named mods this one hides (discovery order wins). */
  shadows: string[];
};

export function listMods(project: string | null): Promise<ModRecord[]> {
  return invoke("mods_list", { project });
}

export function setModEnabled(
  project: string | null,
  name: string,
  enabled: boolean,
  manifestHash: string,
): Promise<void> {
  return invoke("mods_set_enabled", { project, name, enabled, manifestHash });
}

export function addUnpackedMod(path: string): Promise<void> {
  return invoke("mods_add_unpacked", { path });
}

let esbuild: Promise<typeof import("esbuild-wasm")> | null = null;

function loadEsbuild() {
  // No worker: the main CSP has no `worker-src blob:`, on purpose.
  esbuild ??= import("esbuild-wasm").then(async (mod) => {
    await mod.initialize({ wasmURL, worker: false });
    return mod;
  });
  return esbuild;
}

/** Resolves `rel` against the directory of `from`, both relative to the mod folder. */
export function joinModPath(from: string, rel: string): string {
  const parts = from.split("/").slice(0, -1);
  for (const part of rel.split("/")) {
    if (part === "..") parts.pop();
    else if (part && part !== ".") parts.push(part);
  }
  return parts.join("/");
}

const EXTENSIONS = [
  "",
  ".tsx",
  ".ts",
  ".jsx",
  ".js",
  "/index.tsx",
  "/index.ts",
  "/index.js",
];
const SHIMS = /^(monocode|monocode\/jsx-runtime|claude-code)$/;

// ponytail: in-memory per window, not the $APPDATA/<contentHash>.js cache; add it if cold starts get slow.
const bundles = new Map<string, Promise<string>>();

export function forgetBundle(name: string) {
  for (const key of bundles.keys())
    if (key.startsWith(`${name}\0`)) bundles.delete(key);
}

/** The mod's entry and its relative imports, plus the worker runtime, as one IIFE. */
export function bundleMod(
  project: string | null,
  mod: ModRecord,
): Promise<string> {
  const key = `${mod.name}\0${mod.manifestHash}`;
  let bundle = bundles.get(key);
  if (!bundle) {
    bundle = build(project, mod);
    bundles.set(key, bundle);
    bundle.catch(() => bundles.delete(key));
  }
  return bundle;
}

async function build(project: string | null, mod: ModRecord): Promise<string> {
  const { build } = await loadEsbuild();
  const read = (relPath: string) =>
    invoke<string>("mods_read_source", { project, name: mod.name, relPath });
  const plugin: Plugin = {
    name: "monomod",
    setup(b) {
      b.onResolve({ filter: SHIMS }, () => ({
        path: "runtime",
        namespace: "runtime",
      }));
      b.onResolve({ filter: /^\.\.?\// }, (args) => ({
        path: joinModPath(
          args.namespace === "mod" ? args.importer : "",
          args.path,
        ),
        namespace: "mod",
      }));
      b.onResolve({ filter: /.*/ }, (args) => ({
        errors: [
          {
            text: `Only relative imports and "monocode" are supported: ${args.path}`,
          },
        ],
      }));
      b.onLoad({ filter: /.*/, namespace: "runtime" }, () => ({
        contents: runtimeSource,
        loader: "ts",
      }));
      b.onLoad({ filter: /.*/, namespace: "mod" }, async (args) => {
        for (const ext of EXTENSIONS) {
          const path = args.path + ext;
          const contents = await read(path).catch(() => null);
          if (contents === null) continue;
          const loader = /\.tsx$/.test(path)
            ? "tsx"
            : /\.ts$/.test(path)
              ? "ts"
              : /\.json$/.test(path)
                ? "json"
                : "jsx";
          return { contents, loader };
        }
        return { errors: [{ text: `Cannot read ${args.path}` }] };
      });
    },
  };
  const result = await build({
    stdin: {
      contents: `import { boot } from "monocode";\nimport * as mod from "./${mod.manifest.entry}";\nboot(mod.register ?? mod.default);`,
      sourcefile: "entry.ts",
      loader: "ts",
    },
    bundle: true,
    write: false,
    format: "iife",
    target: "es2022",
    jsx: "automatic",
    jsxImportSource: "monocode",
    plugins: [plugin],
    logLevel: "silent",
  });
  return result.outputFiles[0].text;
}
