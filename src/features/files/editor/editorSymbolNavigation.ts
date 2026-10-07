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
  const identifierEnd = String.raw`(?=$|[^A-Za-z0-9_$])`;
  const declaration = new RegExp(
    String.raw`(?:^|\s)(?:export\s+)?(?:async\s+)?(?:function|fn|def|func|class|interface|type|struct|enum)\s+${escaped}${identifierEnd}|(?:^|\s)(?:const|let|var|val)\s+${escaped}${identifierEnd}\s*=|\b${escaped}${identifierEnd}\s*\([^)]*\)\s*(?::[^=]+)?(?:=>|\{)`,
  );
  return (
    matches.find((match) => {
      const declarationMatch = declaration.exec(match.preview);
      return (
        declarationMatch &&
        !isCommentedOut(match.preview, declarationMatch.index)
      );
    }) ?? null
  );
}

/** Escapes a symbol before it is embedded in a declaration-matching regex. */
function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Rejects declarations whose matched text begins within a line comment or block comment. */
function isCommentedOut(line: string, index: number): boolean {
  const prefix = line.slice(0, index);
  if (prefix.includes("//") || prefix.includes("#")) return true;
  return prefix.lastIndexOf("/*") > prefix.lastIndexOf("*/");
}
