import type { ProjectSearchMatch } from "../../search/model/search";

export type EditorSymbol = { name: string; from: number; to: number };

/** Returns the identifier under a cursor, including the `$` used in JS names. */
export function symbolAt(text: string, position: number): EditorSymbol | null {
  const isSymbol = (character: string | undefined) =>
    !!character && /[A-Za-z0-9_$]/.test(character);
  let from = Math.min(Math.max(0, position), text.length);
  let to = from;
  if (!isSymbol(text[from]) && isSymbol(text[from - 1])) from -= 1;
  if (!isSymbol(text[from])) return null;
  while (isSymbol(text[from - 1])) from -= 1;
  while (isSymbol(text[to])) to += 1;
  return { name: text.slice(from, to), from, to };
}

/** Prefer conventional function declarations, then assignments, over usages. */
export function definitionFor(
  name: string,
  matches: readonly ProjectSearchMatch[],
): ProjectSearchMatch | null {
  const escaped = escapeRegex(name);
  const declaration = new RegExp(
    String.raw`(?:^|\s)(?:export\s+)?(?:async\s+)?(?:function|fn|def|func|class|interface|type|struct|enum)\s+${escaped}\b|(?:^|\s)(?:const|let|var|val)\s+${escaped}\s*=|\b${escaped}\s*\([^)]*\)\s*(?::[^=]+)?(?:=>|\{)`,
  );
  return matches.find((match) => declaration.test(match.preview)) ?? null;
}

function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
