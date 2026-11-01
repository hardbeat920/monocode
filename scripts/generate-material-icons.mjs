// Usage: node gen.mjs <react-material-icon-theme dir> <material-icon-theme dir> <out.json> <web-overlay.json>
import { pathToFileURL } from "node:url";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
const [dir, themeDir, out, overlayOut] = process.argv.slice(2);
const m = await import(pathToFileURL(`${dir}/dist/index.esm.js`).href);
const usable = (i) => !i.disabled && !i.enabledFor;
const files = {}, exts = {}, folders = {};
for (const i of m.fileIcons.icons) {
  if (!usable(i)) continue;
  for (const n of i.fileNames ?? []) files[n] ??= i.name;
  for (const e of i.fileExtensions ?? []) exts[e] ??= i.name;
}
const specific = m.folderIcons.find((t) => t.name === "specific");
for (const i of specific.icons) {
  if (i.disabled) continue;
  for (const n of i.folderNames ?? []) {
    const keys = [n, `.${n}`, `_${n}`, `-${n}`, `__${n}__`];
    for (const p of [".", "_", "-"]) if (n.startsWith(p)) keys.push(n.slice(1));
    if (n.startsWith("__") && n.endsWith("__") && n.length > 4) keys.push(n.slice(2, -2));
    for (const k of keys) folders[k] ??= i.name;
  }
}
const used = new Set(["file", "folder", "folder-open", ...Object.values(files), ...Object.values(exts)]);
for (const f of Object.values(folders)) { used.add(f); used.add(`${f}-open`); }
const svgs = {};
// The React package drops open glyphs the upstream theme ships; take those
// from material-icon-theme, the art behind the VS Code extension.
const overlay = {};
for (const n of [...used].sort()) {
  const s = m.getIconSvg(n);
  if (s) { svgs[n] = s; continue; }
  const path = `${themeDir}/icons/${n}.svg`;
  if (existsSync(path)) svgs[n] = overlay[n] = readFileSync(path, "utf8").trim();
}
writeFileSync(out, JSON.stringify({ files, exts, folders, svgs }));
writeFileSync(overlayOut, `${JSON.stringify(overlay, null, 2)}\n`);
console.log(Object.keys(files).length, Object.keys(exts).length, Object.keys(folders).length, Object.keys(svgs).length);
