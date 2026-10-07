import { cpSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

for (const directory of ["cmaps", "standard_fonts", "wasm"]) {
  const source = fileURLToPath(new URL(`../node_modules/pdfjs-dist/${directory}/`, import.meta.url));
  const target = fileURLToPath(new URL(`../public/pdfjs/${directory}/`, import.meta.url));
  mkdirSync(target, { recursive: true });
  cpSync(source, target, { recursive: true });
}
