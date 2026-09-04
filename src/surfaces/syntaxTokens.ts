import {
  ensureSyntaxTree,
  syntaxTree,
} from "@codemirror/language";
import { EditorState, type Extension } from "@codemirror/state";
import { highlightCode } from "@lezer/highlight";
import type { ColorScheme } from "../lib/appearance";
import type { UnifiedBlock, UnifiedLine } from "../lib/unifiedDiff";
import {
  languageForPath,
  syntaxTagHighlighter,
} from "./editorLanguage";

type DiffFile = {
  path: string;
  binary?: boolean;
  tooLarge?: boolean;
  blocks: readonly UnifiedBlock[];
};

export type SyntaxToken = {
  text: string;
  color?: string;
};

const MAX_DIFF_HIGHLIGHT_CHARS = 250_000;
const SYNTAX_TREE_BUDGET_MS = 100;

export function highlightSource(
  text: string,
  language: Extension | null,
  scheme: ColorScheme,
): SyntaxToken[][] {
  if (!text) return [[]];
  if (!language) return unstyledLines(text);

  const state = EditorState.create({
    doc: text,
    extensions: [language],
  });
  const code = state.doc.toString();
  const tree =
    ensureSyntaxTree(state, state.doc.length, SYNTAX_TREE_BUDGET_MS) ??
    syntaxTree(state);
  const highlighter = syntaxTagHighlighter(scheme);
  const lines: SyntaxToken[][] = [[]];
  highlightCode(
    code,
    tree,
    highlighter,
    (piece, color) => {
      if (!piece) return;
      lines[lines.length - 1]?.push({
        text: piece,
        ...(color ? { color } : {}),
      });
    },
    () => {
      lines.push([]);
    },
  );
  return lines;
}

export async function highlightDiffFile(
  file: DiffFile,
  scheme: ColorScheme,
): Promise<Map<UnifiedLine, SyntaxToken[]>> {
  const map = new Map<UnifiedLine, SyntaxToken[]>();
  if (file.binary || file.tooLarge) return map;

  const original: UnifiedLine[] = [];
  const current: UnifiedLine[] = [];
  let originalChars = 0;
  let currentChars = 0;
  for (const block of file.blocks) {
    for (const line of block.lines) {
      if (line.kind === "hunk") continue;
      if (line.kind !== "add") {
        original.push(line);
        originalChars += line.text.length + 1;
      }
      if (line.kind !== "del") {
        current.push(line);
        currentChars += line.text.length + 1;
      }
      // Rendering remains complete; only decorative parsing is skipped.
      if (
        originalChars > MAX_DIFF_HIGHLIGHT_CHARS ||
        currentChars > MAX_DIFF_HIGHLIGHT_CHARS
      ) {
        return map;
      }
    }
  }

  const language = await languageForPath(file.path);

  const originalTokens = highlightSource(
    original.map((line) => line.text).join("\n"),
    language,
    scheme,
  );
  const currentTokens = highlightSource(
    current.map((line) => line.text).join("\n"),
    language,
    scheme,
  );
  assignLineTokens(
    map,
    original,
    originalTokens,
    (line) => line.kind === "del",
  );
  assignLineTokens(map, current, currentTokens, (line) => line.kind !== "del");
  return map;
}

function assignLineTokens(
  map: Map<UnifiedLine, SyntaxToken[]>,
  lines: readonly UnifiedLine[],
  tokens: readonly SyntaxToken[][],
  take: (line: UnifiedLine) => boolean,
) {
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (!take(line)) continue;
    const pieces = tokens[index];
    map.set(
      line,
      pieces && joinText(pieces) === line.text ? pieces : [{ text: line.text }],
    );
  }
}

function unstyledLines(text: string): SyntaxToken[][] {
  return text.split("\n").map((line) => (line ? [{ text: line }] : []));
}

function joinText(tokens: readonly SyntaxToken[]): string {
  return tokens.map((token) => token.text).join("");
}
