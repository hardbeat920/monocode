import { ensureSyntaxTree, syntaxTree } from "@codemirror/language";
import { diagnosticCount, linter, type Diagnostic } from "@codemirror/lint";
import type { EditorState, Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { basename } from "../../../platform/tauri/fs";

/**
 * Syntax diagnostics straight off the Lezer parse tree.
 *
 * The parser already runs for syntax highlighting, and its error recovery
 * leaves error nodes in the tree wherever it could not proceed. Reading those
 * back out costs one tree walk and no new parsing, so this catches the typo
 * class of mistakes — unclosed brackets, stray quotes, malformed statements —
 * without any of the weight of a real type checker or language server.
 *
 * Highlighter grammars are not spec-complete. Known gaps (arrow type
 * predicates, typed `catch`, Tailwind `@source`, …) are filtered so they
 * don't light up as errors. Rust is highlighted but not linted: `@lezer/rust`
 * still misses `let`-`else` and attributes on statements, and those holes
 * cascade through the rest of the file.
 */

/** Mirrors `MAX_FORMAT_CHARS` in lib/format: past this, a file is a viewer. */
const MAX_LINT_CHARS = 512 * 1024;

/**
 * Lezer recovers by resyncing, so one bad character can strand the rest of the
 * file behind a wall of follow-on errors. The first handful are the real ones.
 */
const MAX_DIAGNOSTICS = 50;

/** Long enough to outlast a keystroke burst, short enough to feel live. */
const LINT_DELAY_MS = 700;

const MAX_TOKEN_CHARS = 24;

/** Timebox a full parse so lint doesn't wait on a huge file's last chunk. */
const PARSE_BUDGET_MS = 150;

/**
 * Extensions whose grammar actually reports errors. Keep in sync with
 * `languageForPath` in editorChrome — minus Markdown (accepts anything) and
 * Rust (highlighter grammar is too incomplete to trust).
 */
const LINTABLE_EXTENSIONS = new Set([
  ".js",
  ".jsx",
  ".mjs",
  ".cjs",
  ".ts",
  ".tsx",
  ".json",
  ".css",
  ".html",
  ".htm",
  ".py",
]);

export function editorLint(
  path: string,
  onErrorCount?: (count: number) => void,
): Extension {
  if (!isLintable(path)) return [];
  return [
    linter((view) => syntaxDiagnostics(view.state), {
      delay: LINT_DELAY_MS,
      /**
       * The language arrives in a compartment after an async import, and large
       * files parse in chunks. Neither is a document change, so without this a
       * file opened with an error in it stays clean until the first keystroke.
       */
      needsRefresh: (update) =>
        syntaxTree(update.state) !== syntaxTree(update.startState),
    }),
    onErrorCount ? errorCountReporter(onErrorCount) : [],
    lintTheme,
  ];
}

/**
 * Diagnostics land in the lint state field on the linter's own timer rather
 * than as part of an edit, so a tab that wants to show an error badge has to
 * watch for the field changing underneath it.
 */
function errorCountReporter(onErrorCount: (count: number) => void): Extension {
  let reported = 0;
  return EditorView.updateListener.of((update) => {
    const count = diagnosticCount(update.state);
    if (count === reported) return;
    reported = count;
    onErrorCount(count);
  });
}

export function isLintable(path: string): boolean {
  const name = basename(path).toLowerCase();
  const extension = name.includes(".") ? name.slice(name.lastIndexOf(".")) : "";
  return LINTABLE_EXTENSIONS.has(extension);
}

export function syntaxDiagnostics(state: EditorState): Diagnostic[] {
  if (state.doc.length > MAX_LINT_CHARS) return [];

  // Viewport parsing leaves a dummy error at the frontier (~3kb in). Lint
  // has to finish the tree or it will underline the next `import` forever.
  const tree =
    ensureSyntaxTree(state, state.doc.length, PARSE_BUDGET_MS) ??
    syntaxTree(state);
  if (tree.length === 0) return [];

  const incomplete = tree.length < state.doc.length;
  const diagnostics: Diagnostic[] = [];
  tree.iterate({
    enter: (node) => {
      if (!node.type.isError) return true;
      if (incomplete && node.from >= tree.length) return false;
      if (isGrammarGap(state, node.node.parent, node.from)) return false;
      if (diagnostics.length < MAX_DIAGNOSTICS) {
        diagnostics.push(errorDiagnostic(state, node.from, node.to));
      }
      // Error nodes nest, and stacking the children on top of the parent just
      // paints the same typo several times over.
      return false;
    },
  });
  return diagnostics;
}

/** Arrow `(x): x is T =>`, including tuple predicates like `x is [K, V]`. */
const TYPE_PREDICATE = /:\s*(?:asserts\s+)?(?:this|[\w$]+)\s+is\b/;

/** `catch (error: unknown)` — Lezer's catch clause has no type annotation. */
const TYPED_CATCH = /^catch\s*\(\s*[\w$]+\s*:/;

// Multiline JSX comments: `{` then block-comment then `}`.
const JSX_BLOCK_COMMENT = /^\{\s*\/\*/;

/** Tailwind v4 at-rules the CSS grammar doesn't know. */
const TAILWIND_AT =
  /^@(?:source|plugin|theme|utility|custom-variant|reference|config)\b/;

/** `fn<typeof import("mod")>()` — `import(` is parsed as a dynamic import. */
const TYPEOF_IMPORT = /<typeof\s+import\s*\(/;

function isGrammarGap(
  state: EditorState,
  parent: { name: string; from: number; to: number; parent: unknown } | null,
  from: number,
): boolean {
  const line = state.doc.lineAt(from).text;
  // Tuple predicates (`x is [K, V]`) blow the statement apart, so the `is`
  // token is no longer inside TypeAnnotation. The whole line is the predicate.
  if (TYPE_PREDICATE.test(line) || TYPEOF_IMPORT.test(line)) return true;

  for (; parent; parent = parent.parent as typeof parent) {
    const text = state.doc.sliceString(parent.from, parent.to);
    if (parent.name === "CatchClause" && TYPED_CATCH.test(text)) return true;
    if (parent.name === "JSXEscape" && JSX_BLOCK_COMMENT.test(text)) return true;
    if (parent.name === "AtRule" && TAILWIND_AT.test(text)) return true;
  }
  return false;
}

function errorDiagnostic(
  state: EditorState,
  from: number,
  to: number,
): Diagnostic {
  const message = errorMessage(state, from, to);
  let [start, end] = [from, to];

  // Most error nodes are zero-width insertion points, which render as a small
  // marker. Widening to a character gets the underline that reads as "wrong",
  // but never across a line break — that would underline the whole line.
  if (start === end) {
    const line = state.doc.lineAt(start);
    if (end < line.to) end += 1;
    else if (start > line.from) start -= 1;
  }

  return { from: start, to: end, severity: "error", message };
}

function errorMessage(state: EditorState, from: number, to: number): string {
  const skipped = to > from ? state.doc.sliceString(from, to).trim() : "";
  if (skipped) return `Unexpected ${quote(skipped)}`;

  const token = tokenAt(state, from);
  if (token) return `Unexpected ${quote(token)}`;

  return from >= state.doc.length
    ? "Unexpected end of file"
    : "Unexpected end of line";
}

/** The token the parser choked on: the parse stops just before it. */
function tokenAt(state: EditorState, pos: number): string | null {
  if (pos >= state.doc.length) return null;
  const line = state.doc.lineAt(pos);
  if (pos >= line.to) return null;

  const rest = state.doc.sliceString(
    pos,
    Math.min(line.to, pos + MAX_TOKEN_CHARS + 1),
  );
  return rest.trimStart().match(/^(\w+|[^\s\w])/)?.[1] ?? null;
}

function quote(text: string): string {
  const flat = text.replace(/\s+/g, " ");
  const clipped =
    flat.length > MAX_TOKEN_CHARS ? `${flat.slice(0, MAX_TOKEN_CHARS)}…` : flat;
  return `"${clipped}"`;
}

const lintTheme = EditorView.theme({
  ".cm-lintRange-error": {
    // The base theme paints a wavy line as a data-URI background image, which
    // can only be recoloured by reproducing the whole SVG. A text-decoration
    // squiggle takes the palette directly and renders sharper.
    backgroundImage: "none",
    textDecoration: "underline wavy #f87171",
    textDecorationSkipInk: "none",
    textUnderlineOffset: "3px",
  },
  ".cm-lintPoint:after": {
    borderBottomColor: "#f87171",
  },
  ".cm-tooltip-lint .cm-diagnostic": {
    fontFamily: "var(--font-mono)",
    fontSize: "12px",
    padding: "4px 8px",
    marginLeft: "0",
  },
  ".cm-tooltip-lint .cm-diagnostic-error": {
    borderLeft: "2px solid #f87171",
  },
});
