import fs from "node:fs";

/**
 * Guards the gettext-style table in `src/shared/lib/i18n.ts`: every English
 * literal passed to `t()` must have a Chinese entry, or the string silently
 * renders in English. Run with `node scripts/check-i18n-keys.mjs`.
 */

const SOURCE = "src/shared/lib/i18n.ts";

const files = [
  "src/features/settings/ui/SettingsView.tsx",
  "src/app/shell/TitleBar.tsx",
  "src/app/shell/SettingsRail.tsx",
  "src/features/projects/ui/RemoveProjectDialog.tsx",
  "src/app/shell/MenuBar.tsx",
  "src/app/shell/Sidebar.tsx",
  "src/features/projects/ui/SearchableProjectPicker.tsx",
  "src/features/sessions/ui/Composer.tsx",
  "src/integrations/harness/core/availability.ts",
];

/** Keys in the ZH table, quoted or bare. Duplicates are reported, not merged. */
function readTable(source) {
  const text = fs.readFileSync(source, "utf8");
  const start = text.indexOf("const ZH");
  const end = text.indexOf("const TABLES");
  if (start < 0 || end < 0) throw new Error("ZH table not found in " + source);
  const keys = new Set();
  const duplicates = [];
  const lines = text.slice(start, end).split("\n");
  const offset = text.slice(0, start).split("\n").length;
  lines.forEach((line, index) => {
    const quoted = /^ {2}"((?:[^"\\]|\\.)*)":/.exec(line);
    const bare = /^ {2}([A-Za-z_][A-Za-z0-9_]*):/.exec(line);
    if (!quoted && !bare) return;
    const key = quoted ? quoted[1].replace(/\\(.)/g, "$1") : bare[1];
    if (keys.has(key)) duplicates.push({ key, line: offset + index });
    keys.add(key);
  });
  return { keys, duplicates };
}

/**
 * Literals inside the *first* argument of `t(...)`. The argument list is
 * balanced-scanned first, so ternaries and multi-line keys are covered, then
 * cut at the first top-level comma so parameter values are not mistaken for
 * keys (e.g. `t("Downloading{progress}", { progress: "…" })`).
 */
function readUsedLiterals(source) {
  const text = fs.readFileSync(source, "utf8");
  const found = new Set();
  const call = /\bt\(/g;
  let match;
  while ((match = call.exec(text)) !== null) {
    const open = match.index + 1; // sits on the "("
    let depth = 0;
    let index = open;
    for (; index < text.length; index += 1) {
      const char = text[index];
      if (char === "(") depth += 1;
      else if (char === ")") {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    const args = text.slice(open + 1, index);
    for (const literal of readFirstArgumentLiterals(args)) found.add(literal);
  }
  return found;
}

/** String literals in `args` before the first top-level comma. */
function readFirstArgumentLiterals(args) {
  const literals = new Set();
  let brace = 0;
  let bracket = 0;
  let quote = null;
  let current = "";
  for (let i = 0; i < args.length; i += 1) {
    const char = args[i];
    if (quote) {
      if (char === "\\") {
        current += args[i + 1] ?? "";
        i += 1;
        continue;
      }
      if (char === quote) {
        literals.add(current);
        current = "";
        quote = null;
        continue;
      }
      current += char;
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      continue;
    }
    if (char === "{") brace += 1;
    else if (char === "}") brace -= 1;
    else if (char === "[") bracket += 1;
    else if (char === "]") bracket -= 1;
    else if (char === "," && brace === 0 && bracket === 0) break;
  }
  return literals;
}

const { keys, duplicates } = readTable(SOURCE);
const used = new Map();
for (const file of files) {
  for (const literal of readUsedLiterals(file)) {
    if (!used.has(literal)) used.set(literal, file);
  }
}

console.log(`zh keys: ${keys.size} | t() literals: ${used.size}`);

console.log("\nduplicate keys (typescript rejects these):");
console.log(
  duplicates.length
    ? duplicates.map((d) => `  line ${d.line}: ${JSON.stringify(d.key)}`).join("\n")
    : "  (none)",
);

const missing = [...used].filter(([key]) => !keys.has(key));
console.log("\nmissing from the zh table (render as english):");
console.log(
  missing.length
    ? missing.map(([k, f]) => `  ${f}: ${JSON.stringify(k)}`).join("\n")
    : "  (none)",
);

// Unused entries are prunable, but section labels and dynamic keys are only
// reachable through variables, so treat this as a hint rather than an error.
const unused = [...keys].filter((key) => !used.has(key));
console.log("\nzh keys with no literal call site (verify before pruning):");
console.log(unused.length ? unused.map((k) => `  ${JSON.stringify(k)}`).join("\n") : "  (none)");

process.exitCode = missing.length || duplicates.length ? 1 : 0;
