import { build } from "esbuild";
import { readFile, writeFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import vm from "node:vm";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const asset = path.join(root, "crates/monocode-gpui/assets/native-editor.js");
const options = {
  absWorkingDir: here,
  entryPoints: ["index.ts"],
  outfile: asset,
  bundle: true,
  format: "iife",
  platform: "neutral",
  target: "es2020",
  minify: true,
  nodePaths: [path.join(here, "node_modules")],
  plugins: [{
    name: "native-paths",
    setup(build) {
      // Shared frontend helpers resolve from the repository root. Keep their
      // editor extensions on this bundle's package instances as well.
      build.onResolve({ filter: /^@(codemirror|lezer|replit)\// }, args => ({ path: fileURLToPath(import.meta.resolve(args.path)) }));
      build.onResolve({ filter: /platform\/tauri\/fs$/ }, () => ({ path: "basename", namespace: "native" }));
      build.onLoad({ filter: /.*/, namespace: "native" }, () => ({ contents: 'export const basename = path => path.replace(/\\\\/g, "/").replace(/\\/+$/, "").split("/").pop() || "/";' }));
    },
  }],
};
await build(options);
await build({ ...options, entryPoints: ["format.ts"], outfile: asset.replace("native-editor.js", "native-format.js") });
// No DOM, Node or external services are available in the native interpreter.
const sandbox = vm.createContext({});
vm.runInContext(await readFile(asset, "utf8"), sandbox);
const sample = JSON.parse(await sandbox.monocodeAnalyze("test.json", '{"bad": }', "{}"));
if (!sample.diagnostics.length || !sample.changes.length) throw new Error("Native analysis smoke check failed");
const largeSource = await readFile(path.join(root, "src/app/App.tsx"), "utf8");
const largeAnalysis = JSON.parse(await sandbox.monocodeAnalyze("App.tsx", largeSource, `${largeSource}\nexport const previousHighlightTail = "tail";\n`));
if (!largeAnalysis.tokens.some(token => token.to > largeSource.length - 1024)) throw new Error("Large-file highlighting stops before the end of App.tsx");
if (!largeAnalysis.changes.some(change => change.original.includes("previousHighlightTail") && change.tokens.some(token => token.kind === "var(--syntax-string)"))) throw new Error("Large-file baseline highlighting stops before its final change");
const completion = JSON.parse(await sandbox.monocodeComplete("test.js", "cons", 4, true));
if (!completion.some(option => option.label === "const")) throw new Error("Completion smoke check failed");
const search = JSON.parse(sandbox.monocodeSearch("hello HELLO", "(hello)", "$1!", false, false, true));
if (search.matches.length !== 2 || search.matches[0].insert !== "hello!") throw new Error("Search smoke check failed");
const formatter = vm.createContext({});
vm.runInContext(await readFile(asset.replace("native-editor.js", "native-format.js"), "utf8"), formatter);
if (JSON.parse(await formatter.monocodeFormat("a.json", '{"a":1}', 0)).formatted !== '{ "a": 1 }\n') throw new Error("Format smoke check failed");
const paletteBuild = await build({ ...options, entryPoints: [], stdin: { contents: 'import { WORKSPACE_THEMES } from "../../src/features/settings/model/workspaceThemes"; globalThis.palettes = Object.fromEntries(WORKSPACE_THEMES.map(t => [t.id, { dark: { ...t.dark.syntax, heading: t.dark.heading }, light: { ...t.light.syntax, heading: t.light.heading } }]));', resolveDir: here }, write: false });
const palettes = vm.createContext({});
vm.runInContext(paletteBuild.outputFiles[0].text, palettes);
await writeFile(asset.replace("native-editor.js", "editor-palettes.json"), JSON.stringify(palettes.palettes, null, 2) + "\n");
const notices = [await readFile(path.join(here, "node_modules/prettier/THIRD-PARTY-NOTICES.md"), "utf8")];
for (const scope of ["@codemirror", "@lezer", "@replit"]) {
  for (const name of (await readdir(path.join(here, "node_modules", scope))).sort()) {
    const folder = path.join(here, "node_modules", scope, name);
    const pkg = JSON.parse(await readFile(path.join(folder, "package.json"), "utf8"));
    notices.push(`${pkg.name} ${pkg.version}\n${await readFile(path.join(folder, "LICENSE"), "utf8")}`);
  }
}
for (const name of ["crelt", "style-mod", "w3c-keyname", "prettier"]) {
  const folder = path.join(here, "node_modules", name);
  const pkg = JSON.parse(await readFile(path.join(folder, "package.json"), "utf8"));
  notices.push(`${pkg.name} ${pkg.version}\n${await readFile(path.join(folder, "LICENSE"), "utf8")}`);
}
await writeFile(asset.replace(/\.js$/, ".LICENSE.txt"), notices.join("\n\n"));
console.log(`Built ${path.relative(root, asset)} (${(await readFile(asset)).length} bytes)`);
