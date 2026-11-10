// Reuses the React transcript's artwork. Generated PNGs are bundled as one JSON
// resource, so native rendering never needs a React/SVG tree or JS callbacks.
import { createRequire } from "node:module";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const mobile = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(resolve(mobile, "package.json"));
const ts = require("typescript");
const { chromium } = createRequire(resolve(mobile, "../package.json"))("playwright");
const loaded = new Map();
function load(path) {
  if (loaded.has(path)) return loaded.get(path);
  const module = { exports: {} };
  loaded.set(path, module.exports);
  const code = ts.transpileModule(readFileSync(path, "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  new Function("require", "module", "exports", code)(
    (name) => name.startsWith(".") ? load(resolve(dirname(path), name + ".ts")) : require(name),
    module, module.exports,
  );
  loaded.set(path, module.exports);
  return module.exports;
}
const providers = load(resolve(mobile, "src/features/sessions/providerSvgs.ts"));
const material = resolve(mobile, "node_modules/react-material-icon-theme/src");
const { fileIcons } = load(resolve(material, "core/icons/fileIcons.ts"));
const { iconData } = load(resolve(material, "iconData.ts"));
const names = {}, extensions = {}, files = new Set(["file"]);
for (const icon of fileIcons.icons) {
  if (icon.disabled || (icon.enabledFor && !icon.enabledFor.some((pack) => String(pack) === ""))) continue;
  files.add(icon.name);
  for (const name of icon.fileNames ?? []) names[name.toLowerCase()] ??= icon.name;
  for (const ext of icon.fileExtensions ?? []) extensions[ext.toLowerCase()] ??= icon.name;
}
const items = Object.entries(providers.PROVIDER_SVGS).map(([name, svg]) => ["provider:" + name, svg]);
const iconNames = {
  copy: "Copy01Icon", metrics: "ChartBreakoutSquareIcon", check: "Tick02Icon",
  chevronDown: "ArrowDown01Icon", chevronRight: "ArrowRight01Icon", chat: "BubbleChatIcon",
  lock: "LockIcon", edit: "PencilEdit01Icon", search: "Search01Icon",
  terminal: "ComputerTerminal01Icon", robot: "Robot01Icon", wrench: "Wrench01Icon", minus: "MinusSignIcon",
};
const attr = (name) => name.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
for (const [name, symbol] of Object.entries(iconNames)) {
  const value = require("@hugeicons/core-free-icons/" + symbol);
  const shapes = value.default ?? value;
  const children = shapes.map(([tag, props]) => `<${tag} ${Object.entries({ ...props, strokeWidth: 1.75, stroke: "currentColor" })
    .filter(([key]) => key !== "key").map(([key, v]) => `${attr(key)}="${String(v)}"`).join(" ")}/>`).join("");
  items.push(["icon:" + name, `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none">${children}</svg>`]);
}
for (const name of [...files].sort()) if (iconData[name]) items.push(["file:" + name, iconData[name]]);
const images = {};
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage();
  for (let start = 0; start < items.length; start += 24) {
    const batch = await page.evaluate(async (items) => {
      return await Promise.all(items.map(async ([name, source]) => {
        const svg = source.replace(/currentColor/g, "#ffffff");
        const blob = new Blob([svg], { type: "image/svg+xml" });
        const url = URL.createObjectURL(blob);
        const image = new Image();
        try {
          await new Promise((resolve, reject) => { image.onload = resolve; image.onerror = reject; image.src = url; });
          const canvas = document.createElement("canvas"); canvas.width = 48; canvas.height = 48;
          canvas.getContext("2d").drawImage(image, 0, 0, 48, 48);
          return [name, canvas.toDataURL("image/png").split(",")[1]];
        } finally { URL.revokeObjectURL(url); }
      }));
    }, items.slice(start, start + 24));
    for (const [name, data] of batch) images[name] = data;
  }
} finally { await browser.close(); }
const output = resolve(mobile, "modules/monocode-link/ios/Resources");
mkdirSync(output, { recursive: true });
writeFileSync(resolve(output, "transcript-artwork.json"), JSON.stringify({
  generatedFrom: "providerSvgs.ts, @hugeicons/core-free-icons, react-material-icon-theme",
  monochromeProviders: [...providers.MONOCHROME_PROVIDERS], names, extensions, images,
}) + "\n");
console.log(`Generated native transcript artwork: ${Object.keys(images).length} icons.`);
