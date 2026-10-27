// Usage: node gen.mjs <@hugeicons/core-free-icons dir> <out dir> name=Icon@stroke ...
import { pathToFileURL } from "node:url";
import { writeFileSync } from "node:fs";
const [dir, out, ...specs] = process.argv.slice(2);
const attr = (k) => k.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
for (const spec of specs) {
  const [name, rest] = spec.split("=");
  const [icon, width] = rest.split("@");
  // icons.tsx's stroke-only FoldVertical/UnfoldVertical.
  const dashes = [["path",{d:"M2 12H4"}],["path",{d:"M8 12H10"}],["path",{d:"M14 12H16"}],["path",{d:"M20 12H22"}]];
  const folds = {
    FoldVertical: [...dashes, ["path",{d:"M7 2L12 7L17 2"}], ["path",{d:"M7 22L12 17L17 22"}]],
    UnfoldVertical: [...dashes, ["path",{d:"M7 7L12 2L17 7"}], ["path",{d:"M7 17L12 22L17 17"}]],
  };
  const els = folds[icon]
    ? folds[icon].map(([t,a])=>[t,{...a,stroke:"currentColor",strokeLinecap:"round",strokeLinejoin:"round",strokeWidth:"1.5"}])
    : (await import(pathToFileURL(`${dir}/dist/esm/${icon}.js`).href)).default;
  const body = els.map(([tag, a]) => {
    const parts = Object.entries(a).filter(([k]) => k !== "key").map(([k, v]) => {
      if (k === "stroke" && v === "currentColor") v = "white";
      if (k === "fill" && v === "currentColor") v = "white";
      if (k === "strokeWidth") v = width;
      return `${attr(k)}="${v}"`;
    });
    return `<${tag} ${parts.join(" ")}/>`;
  }).join("");
  writeFileSync(`${out}/${name}.svg`, `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none">${body}</svg>`);
}
